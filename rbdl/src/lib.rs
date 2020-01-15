extern crate proc_macro;
extern crate quote;
extern crate syn;

mod rbdl_syn;

use proc_macro::TokenStream;
use rbdl_syn::*;
use std::fs::File as FsFile;
use std::io::prelude::*;
use std::path::PathBuf;
use syn::{parse_macro_input, LitStr};

#[proc_macro]
pub fn rbdl_file(input: TokenStream) -> TokenStream {
    let definition_file = parse_macro_input!(input as LitStr);
    let mut path = PathBuf::new();
    path.push(env!("CARGO_MANIFEST_DIR"));
    path.push(definition_file.value());
    let mut file = match FsFile::open(&path) {
        Ok(file) => file,
        Err(e) => panic!(format!("{}: {}", &definition_file.value(), e)),
    };
    let mut definitions = String::new();
    if let Err(e) = file.read_to_string(&mut definitions) {
        panic!(format!("{}: {}", &definition_file.value(), e));
    }
    let stream: proc_macro::TokenStream = definitions.parse().unwrap();
    return rbdl(stream);
}

#[proc_macro]
pub fn rbdl(input: TokenStream) -> TokenStream {
    let parse_tree = parse_macro_input!(input as RBDLFile);
    println!("{:#?}", parse_tree);
    return TokenStream::new();
}