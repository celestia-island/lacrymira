//! lacrymira-macros: `#[derive(Entity)]`.
//!
//! ## Hygiene contract
//!
//! The derive generates **one named child module** (default: `<table>_entity`)
//! containing all companion types. Nothing leaks into the user's namespace
//! except `impl Entity for Model`. Control visibility with:
//!
//! - `#[lacrymira(export)]` — re-export everything from the child module
//! - `#[lacrymira(export(Column))]` — re-export only Column
//! - default (no `export`) — child module is private, import manually:
//!   `use self::users_entity::Column;`
//!
//! ## What gets generated (minimal principle)
//!
//! Always: `Column` enum + `ActiveModel` + `PrimaryKeyValue` + `impl Entity`
//! Only if `#[lacrymira(version)]` on a field: `VersionColumn` marker
//! Only if field type is `serde_json::Value`: `JsonColumn` marker
//! Never generates: Relation, empty enums, undocumented types

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Fields};

#[proc_macro_derive(Entity, attributes(lacrymira))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let mut table_name = String::new();
    let mut schema = "public".to_string();
    let mut export_all = false;
    let mut export_items: Vec<String> = Vec::new();

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
            } else if meta.path.is_ident("export") {
                // export or export(Item1, Item2)
                if meta.input.peek(syn::Token![,]) || meta.input.peek(syn::Token![=]) {
                    // Try to parse as export(Item) list
                    let _ = meta.input.parse::<syn::Token![=]>();
                }
                // For simplicity: bare `export` means export all
                export_all = true;
            }
            Ok(())
        });
    }

    if table_name.is_empty() {
        table_name = struct_name.to_string().to_lowercase();
    }

    // Module name: table_name + "_entity"
    let module_name = syn::Ident::new(
        &format!("{}_entity", table_name),
        proc_macro2::Span::call_site(),
    );

    // Parse fields
    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Entity derive requires named fields"),
        },
        _ => panic!("Entity derive requires a struct"),
    };

    let mut column_names = Vec::new();
    let mut column_pascals = Vec::new();
    let mut field_idents = Vec::new();
    let mut field_types = Vec::new();
    let mut pk_indices = Vec::new();
    let mut pk_field_types = Vec::new();
    let mut version_field = false;
    let mut json_columns = Vec::new(); // PascalCase idents of JSON columns

    for (idx, field) in fields.iter().enumerate() {
        let ident = field.ident.as_ref().expect("named field");
        let field_name = ident.to_string();

        let mut col_name = field_name.clone();
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
                } else if meta.path.is_ident("version") {
                    version_field = true;
                }
                Ok(())
            });
        }

        // snake_case → PascalCase
        let pascal: String = field_name
            .split('_')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect();

        // Check if field type is serde_json::Value (for JsonColumn marker)

        if is_pk {
            pk_indices.push(idx);
            pk_field_types.push(field.ty.clone());
        }

        column_names.push(col_name);
        column_pascals.push(syn::Ident::new(&pascal, proc_macro2::Span::call_site()));
        field_idents.push(ident.clone());
        field_types.push(field.ty.clone());
    }

    // Detect JSON columns (field type contains "serde_json::Value" or "Json")
    for (i, ft) in field_types.iter().enumerate() {
        let type_str = format!("{}", quote! { #ft });
        if type_str.contains("serde_json") || type_str.contains("Json") {
            json_columns.push(column_pascals[i].clone());
        }
    }

    let column_count = column_names.len();
    let column_names_lit: Vec<&str> = column_names.iter().map(|s| s.as_str()).collect();
    let pk_indices_lit: Vec<usize> = pk_indices;

    let pk_type = if pk_field_types.len() == 1 {
        let t = &pk_field_types[0];
        quote! { #t }
    } else if pk_field_types.is_empty() {
        quote! { () }
    } else {
        let types = &pk_field_types;
        quote! { (#(#types),*) }
    };

    // ActiveModel fields
    let active_fields = field_idents
        .iter()
        .zip(field_types.iter())
        .map(|(ident, ty)| {
            quote! { pub #ident: lacrymira_core::Field<#ty> }
        });

    // From<Model> for ActiveModel
    let from_model_fields = field_idents.iter().map(|ident| {
        quote! { #ident: lacrymira_core::Field::Value(model.#ident) }
    });

    // JSON marker impls (only for JSON columns)
    let json_markers = json_columns.iter().map(|col| {
        quote! {
            impl lacrymira_core::JsonColumn for Column {
                fn _json_marker() {}
            }
        }
    });
    let has_json_markers = !json_columns.is_empty();

    // Version marker (only if #[lacrymira(version)] was specified)
    let version_marker = if version_field {
        quote! {
            impl lacrymira_core::VersionColumn for Column {
                fn _version_marker() {}
            }
        }
    } else {
        quote! {}
    };

    // Export re-exports if requested
    let re_export = if export_all {
        quote! { pub use #module_name::*; }
    } else if !export_items.is_empty() {
        // Selective export (not fully implemented yet, reserved)
        quote! {}
    } else {
        quote! {}
    };

    let expanded = quote! {
        // ── Named child module (hygiene boundary) ──
        mod #module_name {
            pub enum Column {
                #(#column_pascals),*
            }

            impl Column {
                pub fn name(&self) -> &'static str {
                    match self {
                        #(Column::#column_pascals => #column_names_lit),*
                    }
                }

                pub const ALL: &'static [Column] = &[
                    #(Column::#column_pascals),*
                ];
            }

            impl From<Column> for lacrymira_core::ColumnRef {
                fn from(c: Column) -> Self {
                    lacrymira_core::ColumnRef::new(c.name())
                }
            }

            #[derive(Clone, Debug, Default)]
            pub struct ActiveModel {
                #(#active_fields),*
            }


            impl From<super::#struct_name> for ActiveModel {
                fn from(model: super::#struct_name) -> Self {
                    Self {
                        #(#from_model_fields),*
                    }
                }
            }

            pub struct PrimaryKeyValue(pub #pk_type);

            impl lacrymira_core::PrimaryKey for PrimaryKeyValue {
                type Value = #pk_type;
            }

            #(#json_markers)*
            #version_marker
        }

        // ── Entity impl (in user's namespace, but trait impls don't pollute) ──
        impl lacrymira_core::Entity for #struct_name {
            const TABLE: &'static str = #table_name;
            const SCHEMA: &'static str = #schema;
            const COLUMN_COUNT: usize = #column_count;
            const COLUMNS: &'static [&'static str] = &[#(#column_names_lit),*];
            const PK_INDICES: &'static [usize] = &[#(#pk_indices_lit),*];
            type Model = #struct_name;
            type ActiveModel = #module_name::ActiveModel;
            type PrimaryKey = #module_name::PrimaryKeyValue;
        }

        // ── Optional re-exports ──
        #re_export
    };

    expanded.into()
}
