use inflector::cases::pascalcase::to_pascal_case;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Attribute, Data, DataEnum, DataStruct, Field, Fields, FieldsNamed, FieldsUnnamed, GenericParam,
    Lit, LitStr, Meta, MetaList, MetaNameValue, NestedMeta, Path, TypeParam, Variant,
};

/// Derives `PdfFormat`.
///
/// Structs with named fields are written as dictionaries, newtype structs as their field, and
/// enums as the value of their variant (unit variants as names).
///
/// Struct attributes:
/// - `#[rename("Name")]`: the value of the `/Type` key (default: the struct name).
/// - `#[omit_type(true)]`: don't write a `/Type` key.
/// - `#[constant(Key = "Value")]`: write the fixed entry `/Key /Value` after `/Type`, for example
///   `#[constant(Subtype = "Link")]`. Can be repeated.
///
/// Field attributes:
/// - `#[rename("Key")]`: the dictionary key (default: the field name in PascalCase).
/// - `#[skip_if("path")]` or `#[skip_if(path)]`: don't write the entry if `path(&field)` is true.
/// - `#[flatten]`: write the entries of the field (an iterable of key-value pairs) into this
///   dictionary.
#[proc_macro_derive(PdfFormat, attributes(rename, skip_if, omit_type, constant, flatten))]
pub fn pdf_format_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = proc_macro2::TokenStream::from(input);

    // Construct a representation of Rust code as a syntax tree
    // that we can manipulate
    let ast = syn::parse2(input).unwrap();

    // Build the trait implementation
    let result = impl_pdf_format_derive(&ast);

    result.into()
}

fn impl_pdf_format_derive(ast: &syn::DeriveInput) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let attr = parse_attributes(&ast.attrs);

    let generics: &Vec<_> = &ast
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(TypeParam { ident, .. }) => Some(ident),
            _ => None,
        })
        .collect();
    let impl_line = if generics.is_empty() {
        quote! { impl lemon_pdf::PdfFormat for #name }
    } else {
        quote! { impl<#(#generics: lemon_pdf::PdfFormat),*> lemon_pdf::PdfFormat for #name<#(#generics),*> }
    };

    match &ast.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(FieldsNamed { named, .. }),
            ..
        }) => {
            let key_values = named.into_iter().filter_map(|field| {
                let attr = parse_attributes(&field.attrs);
                field.ident.as_ref().map(|ident| {
                    let string = attr
                        .rename
                        .unwrap_or_else(|| to_pascal_case(&ident.to_string()));
                    let key_value = if attr.flatten {
                        quote! {
                            for (key, value) in &self.#ident {
                                f = f.key_value(key, value);
                            }
                        }
                    } else {
                        quote! {
                            f = f.key_value(&#string, &self.#ident);
                        }
                    };
                    if let Some(skip_if) = attr.skip_if {
                        quote! {
                            if !#skip_if(&self.#ident) {
                                #key_value
                            }
                        }
                    } else {
                        key_value
                    }
                })
            });

            let type_key = if attr.omit_type {
                quote! {}
            } else {
                let name_str = attr
                    .rename
                    .unwrap_or_else(|| to_pascal_case(&name.to_string()));
                quote! { f = f.key_value(&"Type", &#name_str); }
            };
            let constants = attr.constants.iter().map(|(key, value)| {
                quote! { f = f.key_value(&#key, &#value); }
            });

            quote! {
                #impl_line {
                    fn write(&self, f: &mut lemon_pdf::object::Formatter) -> std::io::Result<()> {
                        let mut f = f.format_dictionary();
                        #type_key
                        #(#constants)*
                        #(#key_values)*
                        f.finish()
                    }
                }
            }
        }
        Data::Struct(DataStruct {
            fields: Fields::Unnamed(FieldsUnnamed { unnamed, .. }),
            ..
        }) if unnamed.len() == 1 => {
            quote! {
                #impl_line {
                    fn write(&self, f: &mut lemon_pdf::object::Formatter) -> std::io::Result<()> {
                        self.0.write(f)
                    }
                }
            }
        }
        Data::Enum(DataEnum { variants, .. }) => {
            let variants = variants
                .into_iter()
                .map(|Variant { ident, fields, .. }| match fields {
                    Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => {
                        let mut generated_field_names = vec![];
                        let mut generated_code = vec![];
                        for (index, Field { ty, .. }) in unnamed.into_iter().enumerate() {
                            let field_name =
                                syn::Ident::new(&format!("val{}", index), Span::call_site());
                            if index > 0 {
                                generated_code.push(quote! { write!(f, " ")?; });
                            }
                            generated_code.push(quote! { <#ty as lemon_pdf::PdfFormat>::write(#field_name, f)?; });
                            generated_field_names.push(field_name);
                        }
                        quote! {
                            #name::#ident(#(#generated_field_names),*) => { #(#generated_code)* Ok(()) }
                        }
                    }
                    Fields::Named(FieldsNamed { named, .. }) => {
                        let mut field_names = vec![];
                        let mut generated_code = vec![];
                        for (index, Field { ty, ident, .. }) in named.into_iter().enumerate() {
                            let field_name = ident.as_ref().unwrap();
                            if index > 0 {
                                generated_code.push(quote! { write!(f, " ")?; });
                            }
                            generated_code.push(quote! { <#ty as lemon_pdf::PdfFormat>::write(#field_name, f)?; });
                            field_names.push(field_name);
                        }
                        quote! {
                            #name::#ident { #(#field_names),* } => { #(#generated_code)* Ok(()) }
                        }
                    }
                    Fields::Unit => {
                        let variant_name = LitStr::new(&ident.to_string(), ident.span());
                        quote! {
                            #name::#ident => <&'static str as lemon_pdf::PdfFormat>::write(&#variant_name, f)
                        }
                    }
                });
            quote! {
                #impl_line {
                    fn write(&self, f: &mut lemon_pdf::object::Formatter) -> std::io::Result<()> {
                        use std::io::Write;
                        match self {
                            #(#variants),*
                        }
                    }
                }
            }
        }
        _ => panic!("#[derive(PdfFormat)] can only be applied to `struct`s with named fields."),
    }
}

#[derive(Default)]
struct StructAttr {
    omit_type: bool,
    flatten: bool,
    rename: Option<String>,
    skip_if: Option<Path>,
    constants: Vec<(String, String)>,
}

/// Parses the attributes of this derive. Malformed attributes are a compile error instead of
/// being ignored silently.
fn parse_attributes(attributes: &[Attribute]) -> StructAttr {
    let mut result = StructAttr::default();
    for attribute in attributes {
        let name = match attribute.path.segments.last() {
            Some(segment) => segment.value().ident.to_string(),
            None => continue,
        };
        match name.as_str() {
            "rename" => result.rename = Some(string_argument(attribute, &name)),
            "omit_type" => match attribute.parse_meta() {
                Ok(Meta::List(MetaList { nested, .. })) => match nested.iter().next() {
                    Some(NestedMeta::Literal(Lit::Bool(value))) => result.omit_type = value.value,
                    _ => panic!("expected `#[omit_type(true)]`"),
                },
                _ => panic!("expected `#[omit_type(true)]`"),
            },
            "flatten" => match attribute.parse_meta() {
                Ok(Meta::Word(_)) => result.flatten = true,
                _ => panic!("expected `#[flatten]`"),
            },
            "skip_if" => {
                // `skip_if("Vec::is_empty")` or `skip_if(Vec::is_empty)`
                let path = match syn::parse2::<ParenthesizedPath>(attribute.tts.clone()) {
                    Ok(ParenthesizedPath(path)) => path,
                    Err(_) => syn::parse_str(&string_argument(attribute, &name))
                        .expect("`skip_if` expects a path"),
                };
                result.skip_if = Some(path)
            }
            "constant" => match attribute.parse_meta() {
                Ok(Meta::List(MetaList { nested, .. })) => {
                    for nested in nested.iter() {
                        match nested {
                            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                                ident,
                                lit: Lit::Str(value),
                                ..
                            })) => result.constants.push((ident.to_string(), value.value())),
                            _ => panic!("expected `#[constant(Key = \"Value\")]`"),
                        }
                    }
                }
                _ => panic!("expected `#[constant(Key = \"Value\")]`"),
            },
            _ => {}
        }
    }
    result
}

fn string_argument(attribute: &Attribute, name: &str) -> String {
    match attribute.parse_meta() {
        Ok(Meta::List(MetaList { nested, .. })) => match nested.iter().next() {
            Some(NestedMeta::Literal(Lit::Str(value))) => value.value(),
            _ => panic!("expected `#[{}(\"...\")]`", name),
        },
        _ => panic!("expected `#[{}(\"...\")]`", name),
    }
}

/// `(some::path)`
struct ParenthesizedPath(Path);

impl syn::parse::Parse for ParenthesizedPath {
    fn parse(input: syn::parse::ParseStream) -> syn::parse::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let path = content.parse()?;
        if !content.is_empty() {
            return Err(content.error("expected a path"));
        }
        Ok(ParenthesizedPath(path))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
