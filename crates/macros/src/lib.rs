//! lacrymira-macros: `#[derive(Entity)]`.
//!
//! Generates from a `Model` struct:
//! - `impl Entity` (table metadata)
//! - `enum Column` (column references)
//! - `struct ActiveModel` (with `Field<T>` three-state fields)
//! - `impl From<Model> for ActiveModel`
//! - `impl FromRow for Model` (delegating to `sqlx::FromRow` derive)

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

/// Derive the `Entity` implementation from a Model struct.
///
/// ```ignore
/// #[derive(Entity, sqlx::FromRow)]
/// #[lacrymira(table = "billing_ledger")]
/// pub struct Model {
///     #[lacrymira(primary_key)]
///     pub id: i64,
///     pub user_id: Uuid,
///     #[lacrymira(column = "entry_type")]
///     pub entry_type: String,
/// }
/// ```
#[proc_macro_derive(Entity, attributes(lacrymira))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let mut table_name = String::new();
    let mut schema = "public".to_string();

    // Parse attributes using parse_nested_meta
    for attr in &input.attrs {
        if !attr.path().is_ident("lacrymira") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                table_name = lit.value();
            } else if meta.path.is_ident("schema") {
                let value = meta.value()?;
                let lit: syn::LitStr = value.parse()?;
                schema = lit.value();
            }
            Ok(())
        });
    }

    if table_name.is_empty() {
        table_name = struct_name.to_string().to_lowercase();
    }

    // Parse fields
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Entity derive requires named fields"),
        },
        _ => panic!("Entity derive requires a struct"),
    };

    let mut column_names = Vec::new();
    let mut column_idents = Vec::new();
    let mut field_idents = Vec::new();
    let mut field_types = Vec::new();
    let mut pk_indices = Vec::new();
    let mut pk_field_types = Vec::new();

    for (idx, field) in fields.iter().enumerate() {
        let ident = field.ident.as_ref().expect("named field");
        let field_name = ident.to_string();

        // Check for column rename
        let mut col_name = field_name.clone();
        // Convert snake_case to snake_case (no change for now; PG convention)
        let mut is_pk = false;

        for attr in &field.attrs {
            if !attr.path().is_ident("lacrymira") {
                continue;
            }
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("primary_key") {
                    is_pk = true;
                } else if meta.path.is_ident("column") {
                    let value = meta.value()?;
                    let lit: syn::LitStr = value.parse()?;
                    col_name = lit.value();
                }
                Ok(())
            });
        }

        if is_pk {
            pk_indices.push(idx);
            pk_field_types.push(field.ty.clone());
        }

        column_names.push(col_name);
        // snake_case → PascalCase: entry_type → EntryType
        let pascal = field_name
            .split('_')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<String>();
        column_idents.push(syn::Ident::new(&pascal, proc_macro2::Span::call_site()));
        field_idents.push(ident.clone());
        field_types.push(field.ty.clone());
    }

    let column_count = column_names.len();
    let column_names_lit: Vec<&str> = column_names.iter().map(|s| s.as_str()).collect();
    let pk_indices_lit: Vec<usize> = pk_indices;

    // Generate the Column enum
    let column_enum_variants = &column_idents;

    // Determine PK type (single or composite)
    let pk_type = if pk_field_types.len() == 1 {
        let t = &pk_field_types[0];
        quote! { #t }
    } else if pk_field_types.is_empty() {
        quote! { () }
    } else {
        let types = &pk_field_types;
        quote! { (#(#types),*) }
    };

    // Generate ActiveModel fields (each Field<T>)
    let active_fields = field_idents
        .iter()
        .zip(field_types.iter())
        .map(|(ident, ty)| {
            quote! { pub #ident: lacrymira_core::Field<#ty> }
        });

    // Generate From<Model> for ActiveModel
    let from_model_fields = field_idents.iter().map(|ident| {
        quote! { #ident: lacrymira_core::Field::Value(model.#ident) }
    });

    let expanded = quote! {
        // ── Column enum ──
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Column {
            #(#column_enum_variants),*
        }

        impl Column {
            /// The SQL column name for this variant.
            pub fn name(&self) -> &'static str {
                match self {
                    #(Column::#column_enum_variants => #column_names_lit),*
                }
            }

            /// All variants in declaration order.
            pub const ALL: &'static [Column] = &[
                #(Column::#column_enum_variants),*
            ];
        }

        // ── ActiveModel ──
        #[derive(Debug, Clone, Default)]
        pub struct ActiveModel {
            #(#active_fields),*
        }

        impl From<#struct_name> for ActiveModel {
            fn from(model: #struct_name) -> Self {
                Self {
                    #(#from_model_fields),*
                }
            }
        }

        // ── PrimaryKey ──
        #[derive(Debug, Clone, PartialEq)]
        pub struct PrimaryKeyValue(pub #pk_type);

        impl lacrymira_core::PrimaryKey for PrimaryKeyValue {
            type Value = #pk_type;
        }

        // ── Entity impl ──
        impl lacrymira_core::Entity for #struct_name {
            const TABLE: &'static str = #table_name;
            const SCHEMA: &'static str = #schema;
            const COLUMN_COUNT: usize = #column_count;
            const COLUMNS: &'static [&'static str] = &[#(#column_names_lit),*];
            const PK_INDICES: &'static [usize] = &[#(#pk_indices_lit),*];
            type Model = #struct_name;
            type ActiveModel = ActiveModel;
            type PrimaryKey = PrimaryKeyValue;
        }
    };

    expanded.into()
}
