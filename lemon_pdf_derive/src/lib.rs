extern crate proc_macro;

use inflector::cases::pascalcase::to_pascal_case;
use quote::quote;
use syn::{
    Attribute, Data, DataEnum, DataStruct, Field, Fields, FieldsNamed, FieldsUnnamed, GenericParam,
    Lit, LitStr, Meta, MetaList, NestedMeta, Path, TypeParam, Variant,
};

#[proc_macro_derive(PdfFormat, attributes(rename, skip_if))]
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
        quote! { impl PdfFormat for #name }
    } else {
        quote! { impl<#(#generics: PdfFormat),*> PdfFormat for #name<#(#generics),*> }
    };

    match &ast.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(FieldsNamed { named, .. }),
            ..
        }) => {
            let key_values = named.into_iter().filter_map(|field| {
                let attr = parse_attributes(&field.attrs);
                field.ident.as_ref().map(|ident| {
                    let string = attr.rename.unwrap_or_else(|| to_pascal_case(&ident.to_string()));
                    let key_value = quote! {
                        f = f.key_value(&#string, &self.#ident);
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

            let name_str = attr.rename.unwrap_or_else(|| to_pascal_case(&name.to_string()));
            quote! {
                #impl_line {
                    fn write(&self, f: &mut crate::object::Formatter) -> std::io::Result<()> {
                        let mut f = f.format_dictionary().key_value(&"Type", &#name_str);
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
                    fn write(&self, f: &mut crate::object::Formatter) -> std::io::Result<()> {
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
                        assert_eq!(
                            unnamed.len(),
                            1,
                            "Variant must not contain multiple fields."
                        );
                        let Field { ty, .. } = unnamed.into_iter().next().unwrap();
                        quote! {
                            #name::#ident(val) => <#ty as PdfFormat>::write(val, f)
                        }
                    }
                    Fields::Unit => {
                        let variant_name = LitStr::new(&ident.to_string(), ident.span());
                        quote! {
                            #name::#ident => <&'static str as PdfFormat>::write(&#variant_name, f)
                        }
                    }
                    _ => panic!("Variant must not contain named fields."),
                });
            quote! {
                #impl_line {
                    fn write(&self, f: &mut crate::object::Formatter) -> std::io::Result<()> {
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
    rename: Option<String>,
    skip_if: Option<Path>,
}

impl StructAttr {
    fn rename(val: String) -> Self {
        StructAttr {
            rename: Some(val),
            ..Default::default()
        }
    }

    fn skip_if(val: Path) -> Self {
        StructAttr {
            skip_if: Some(val),
            ..Default::default()
        }
    }

    fn or(self, other: StructAttr) -> Self {
        StructAttr {
            rename: self.rename.or(other.rename),
            skip_if: self.skip_if.or(other.skip_if),
        }
    }
}

fn parse_attributes(attribute: &[Attribute]) -> StructAttr {
    attribute
        .iter()
        .map(Attribute::parse_meta)
        .filter_map(|meta| {
            meta.ok().and_then(|meta| match meta {
                Meta::List(MetaList { ident, nested, .. }) => {
                    if ident == "rename" {
                        match nested.iter().next() {
                            Some(NestedMeta::Literal(Lit::Str(str_lit))) => {
                                Some(StructAttr::rename(str_lit.value()))
                            }
                            _ => None,
                        }
                    } else if ident == "skip_if" {
                        match nested.iter().next() {
                            Some(NestedMeta::Literal(Lit::Str(str_lit))) => {
                                Some(StructAttr::skip_if(str_lit.parse().unwrap()))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            })
        })
        .fold(StructAttr::default(), |a, b| a.or(b))
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
