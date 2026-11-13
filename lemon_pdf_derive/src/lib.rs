use inflector::cases::pascalcase::to_pascal_case;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Fields, FieldsNamed, FieldsUnnamed,
    GenericParam, Ident, Lit, Meta, MetaList, MetaNameValue, NestedMeta, Path, Type, TypeParam,
    TypePath, Variant,
};

/// Derives `PdfFormat`: the type is written as a direct object.
///
/// - Structs with named fields are written as dictionaries. They also get an implementation of
///   `PdfDictionary`, so they can be flattened into other dictionaries.
/// - Newtype structs are written as their field.
/// - Enums are written as the value of their variant. Unit variants are written as names;
///   every other variant must have exactly one field, so that every value is one PDF object.
///
/// Struct attributes:
/// - `#[rename("Name")]`: the value of the `/Type` key (default: the struct name).
/// - `#[omit_type(true)]`: don't write a `/Type` key.
/// - `#[constant(Key = "Value")]`: write the fixed entry `/Key /Value` after `/Type`, for example
///   `#[constant(Subtype = "Link")]`. A number is written as a number: `#[constant(FunctionType
///   = 2)]` gives `/FunctionType 2`. Can be repeated.
///
/// Field attributes:
/// - `#[rename("Key")]`: the dictionary key (default: the field name in PascalCase).
/// - `#[skip_if("path")]` or `#[skip_if(path)]`: don't write the entry if `path(&field)` is true.
///   Fields of type `Option<T>` are skipped when they are `None` without this attribute.
/// - `#[flatten]`: write the entries of the field (a `PdfDictionary`) into this dictionary.
///
/// Enum attributes:
/// - `#[indirect_variants]`: implement `From<IndirectReference<Variant>>` for
///   `IndirectReference<Enum>`, so that a reference to a variant can be used where a reference
///   to the enum is expected.
#[proc_macro_derive(
    PdfFormat,
    attributes(rename, skip_if, omit_type, constant, flatten, indirect_variants)
)]
pub fn pdf_format_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast: DeriveInput = syn::parse2(TokenStream::from(input)).unwrap();
    match &ast.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => {
            let dictionary = impl_pdf_dictionary(&ast, fields);
            let format = impl_header(
                &ast,
                quote!(lemon_pdf::PdfFormat),
                quote!(lemon_pdf::PdfFormat),
            );
            quote! {
                #dictionary
                #format {
                    fn write(&self, f: &mut lemon_pdf::object::Formatter) -> std::io::Result<()> {
                        lemon_pdf::object::PdfDictionary::write_entries(self, f.format_dictionary())
                            .finish()
                    }
                }
            }
        }
        Data::Struct(DataStruct {
            fields: Fields::Unnamed(FieldsUnnamed { unnamed, .. }),
            ..
        }) if unnamed.len() == 1 => {
            let format = impl_header(
                &ast,
                quote!(lemon_pdf::PdfFormat),
                quote!(lemon_pdf::PdfFormat),
            );
            quote! {
                #format {
                    fn write(&self, f: &mut lemon_pdf::object::Formatter) -> std::io::Result<()> {
                        lemon_pdf::PdfFormat::write(&self.0, f)
                    }
                }
            }
        }
        Data::Enum(data) => impl_enum_pdf_format(&ast, data),
        _ => panic!(
            "#[derive(PdfFormat)] can only be applied to structs with named fields, newtype \
             structs and enums"
        ),
    }
    .into()
}

/// Derives `PdfDictionary` for a struct with named fields: the entries of a dictionary that is
/// not a complete direct object on its own, like the dictionary of a stream
/// (`Stream<YourStruct>`). Takes the same attributes as `#[derive(PdfFormat)]`.
#[proc_macro_derive(
    PdfDictionary,
    attributes(rename, skip_if, omit_type, constant, flatten)
)]
pub fn pdf_dictionary_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast: DeriveInput = syn::parse2(TokenStream::from(input)).unwrap();
    match &ast.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => impl_pdf_dictionary(&ast, fields).into(),
        _ => panic!("#[derive(PdfDictionary)] can only be applied to structs with named fields"),
    }
}

/// Derives `IndirectObject` for an enum whose variants have one field each, all of them
/// streams (or all of them direct objects). Use it for enums of streams, like the kinds of
/// XObjects; enums of direct objects derive `PdfFormat` instead. Takes `#[indirect_variants]`.
#[proc_macro_derive(IndirectObject, attributes(indirect_variants))]
pub fn indirect_object_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast: DeriveInput = syn::parse2(TokenStream::from(input)).unwrap();
    let data = match &ast.data {
        Data::Enum(data) => data,
        _ => panic!("#[derive(IndirectObject)] can only be applied to enums"),
    };
    if !ast.generics.params.is_empty() {
        panic!("#[derive(IndirectObject)] does not support generic enums");
    }
    let name = &ast.ident;
    let variants = single_field_variants(name, data, "IndirectObject");
    let types: Vec<&Type> = variants.iter().map(|(_, ty)| *ty).collect();
    let first = types
        .first()
        .expect("#[derive(IndirectObject)] needs at least one variant");
    let arms = variants.iter().map(|(ident, _)| {
        quote! { #name::#ident(value) => lemon_pdf::object::IndirectObject::write_indirect(value, f) }
    });
    let message = format!(
        "the variants of `{}` must either all be streams or all be direct objects",
        name
    );
    let conversions = reference_conversions(&ast, &variants);
    let assertions = types.iter().map(|ty| {
        quote! {
            assert!(
                <#ty as lemon_pdf::object::IndirectObject>::IS_STREAM
                    == <#first as lemon_pdf::object::IndirectObject>::IS_STREAM,
                #message
            );
        }
    });
    quote! {
        impl lemon_pdf::object::IndirectObject for #name {
            const IS_STREAM: bool = <#first as lemon_pdf::object::IndirectObject>::IS_STREAM;

            fn write_indirect(&self, f: &mut lemon_pdf::object::Formatter) -> std::io::Result<()> {
                match self {
                    #(#arms),*
                }
            }
        }

        const _: () = {
            #(#assertions)*
        };

        #conversions
    }
    .into()
}

/// `impl<T: bound> trait_path for Name<T>`
fn impl_header(ast: &DeriveInput, trait_path: TokenStream, bound: TokenStream) -> TokenStream {
    let name = &ast.ident;
    let generics: Vec<&Ident> = ast
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(TypeParam { ident, .. }) => Some(ident),
            _ => None,
        })
        .collect();
    if generics.is_empty() {
        quote! { impl #trait_path for #name }
    } else {
        let params = generics.clone();
        let bounds = generics.iter().map(|_| bound.clone());
        quote! { impl<#(#params: #bounds),*> #trait_path for #name<#(#generics),*> }
    }
}

fn is_option(ty: &Type) -> bool {
    match ty {
        Type::Path(TypePath { qself: None, path }) => path
            .segments
            .last()
            .map_or(false, |segment| segment.value().ident == "Option"),
        _ => false,
    }
}

fn impl_pdf_dictionary(ast: &DeriveInput, fields: &FieldsNamed) -> TokenStream {
    let name = &ast.ident;
    let attr = parse_attributes(&ast.attrs);

    let entries = fields.named.iter().map(|field| {
        let field_attr = parse_attributes(&field.attrs);
        let ident = field.ident.as_ref().unwrap();
        let key = field_attr
            .rename
            .clone()
            .unwrap_or_else(|| to_pascal_case(&ident.to_string()));
        let entry = if field_attr.flatten {
            quote! { f = lemon_pdf::object::PdfDictionary::write_entries(&self.#ident, f); }
        } else if is_option(&field.ty) {
            quote! {
                if let Some(value) = &self.#ident {
                    f = f.key_value(#key, value);
                }
            }
        } else {
            quote! { f = f.key_value(#key, &self.#ident); }
        };
        match field_attr.skip_if {
            Some(skip_if) => quote! {
                if !#skip_if(&self.#ident) {
                    #entry
                }
            },
            None => entry,
        }
    });

    let type_key = if attr.omit_type {
        quote! {}
    } else {
        let type_name = attr
            .rename
            .clone()
            .unwrap_or_else(|| to_pascal_case(&name.to_string()));
        quote! { f = f.key_value("Type", &lemon_pdf::object::Name::from_static(#type_name)); }
    };
    let constants = attr.constants.iter().map(|(key, value)| match value {
        ConstantValue::Name(value) => {
            quote! { f = f.key_value(#key, &lemon_pdf::object::Name::from_static(#value)); }
        }
        ConstantValue::Integer(value) => {
            quote! { f = f.key_value(#key, &#value); }
        }
    });

    let header = impl_header(
        ast,
        quote!(lemon_pdf::object::PdfDictionary),
        quote!(lemon_pdf::PdfFormat),
    );
    quote! {
        #header {
            #[allow(unused_mut)]
            fn write_entries<'a, 'b>(
                &self,
                mut f: lemon_pdf::object::DictionaryFormatter<'a, 'b>,
            ) -> lemon_pdf::object::DictionaryFormatter<'a, 'b> {
                #type_key
                #(#constants)*
                #(#entries)*
                f
            }
        }
    }
}

/// The variants (name and the type of the field) of an enum whose variants all have exactly one
/// unnamed field.
fn single_field_variants<'a>(
    name: &Ident,
    data: &'a DataEnum,
    derive: &str,
) -> Vec<(&'a Ident, &'a Type)> {
    data.variants
        .iter()
        .map(|Variant { ident, fields, .. }| match fields {
            Fields::Unnamed(FieldsUnnamed { unnamed, .. }) if unnamed.len() == 1 => {
                (ident, &unnamed.first().unwrap().value().ty)
            }
            _ => panic!(
                "#[derive({})]: the variant `{}::{}` must have exactly one unnamed field",
                derive, name, ident
            ),
        })
        .collect()
}

/// `From<IndirectReference<Variant>> for IndirectReference<Enum>` for every variant, if the enum
/// has the `#[indirect_variants]` attribute.
fn reference_conversions(ast: &DeriveInput, variants: &[(&Ident, &Type)]) -> TokenStream {
    if !parse_attributes(&ast.attrs).indirect_variants {
        return quote! {};
    }
    if !ast.generics.params.is_empty() {
        panic!("#[indirect_variants] does not support generic enums");
    }
    let name = &ast.ident;
    let conversions = variants.iter().map(|(_, ty)| {
        quote! {
            impl From<lemon_pdf::object::IndirectReference<#ty>>
                for lemon_pdf::object::IndirectReference<#name>
            {
                fn from(reference: lemon_pdf::object::IndirectReference<#ty>) -> Self {
                    reference.convert()
                }
            }
        }
    });
    quote! { #(#conversions)* }
}

fn impl_enum_pdf_format(ast: &DeriveInput, data: &DataEnum) -> TokenStream {
    let name = &ast.ident;
    let mut with_field = Vec::new();
    let arms: Vec<TokenStream> = data
        .variants
        .iter()
        .map(|variant| {
            let ident = &variant.ident;
            match &variant.fields {
                Fields::Unit => {
                    let variant_name = ident.to_string();
                    quote! { #name::#ident => lemon_pdf::object::write_name(f, #variant_name) }
                }
                Fields::Unnamed(FieldsUnnamed { unnamed, .. }) if unnamed.len() == 1 => {
                    with_field.push((ident, &unnamed.first().unwrap().value().ty));
                    quote! { #name::#ident(value) => lemon_pdf::PdfFormat::write(value, f) }
                }
                _ => panic!(
                    "#[derive(PdfFormat)]: the variant `{}::{}` must be a unit variant or have \
                     exactly one unnamed field, so that it is written as one PDF object",
                    name, ident
                ),
            }
        })
        .collect();
    if parse_attributes(&ast.attrs).indirect_variants && with_field.len() != arms.len() {
        panic!("#[indirect_variants] requires every variant to have one field");
    }
    let conversions = reference_conversions(ast, &with_field);
    let header = impl_header(
        ast,
        quote!(lemon_pdf::PdfFormat),
        quote!(lemon_pdf::PdfFormat),
    );
    quote! {
        #header {
            fn write(&self, f: &mut lemon_pdf::object::Formatter) -> std::io::Result<()> {
                match self {
                    #(#arms),*
                }
            }
        }

        #conversions
    }
}

#[derive(Default)]
struct StructAttr {
    omit_type: bool,
    flatten: bool,
    indirect_variants: bool,
    rename: Option<String>,
    skip_if: Option<Path>,
    constants: Vec<(String, ConstantValue)>,
}

/// The value of a `#[constant(...)]` entry.
enum ConstantValue {
    Name(String),
    Integer(i64),
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
            "indirect_variants" => match attribute.parse_meta() {
                Ok(Meta::Word(_)) => result.indirect_variants = true,
                _ => panic!("expected `#[indirect_variants]`"),
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
                            })) => result
                                .constants
                                .push((ident.to_string(), ConstantValue::Name(value.value()))),
                            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                                ident,
                                lit: Lit::Int(value),
                                ..
                            })) => result.constants.push((
                                ident.to_string(),
                                ConstantValue::Integer(value.value() as i64),
                            )),
                            _ => panic!("expected `#[constant(Key = \"Value\")]` or `#[constant(Key = 1)]`"),
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
