//! Utility macro to perform token based substitution on the item it is
//! applied to.
//! 
//! # Usage
//! The [`replace`] macro takes two arguments:
//! - an ident to search for, and
//! - the set of tokens to replace it with.
//! 
//! This looks something like this
//! ```
//! # use tokenreplace::replace;
//! # struct Dummy { a: usize, b: usize }
//! # impl Dummy { fn new() -> Self { Self { a: 0, b: 1 } } }
//! #[replace(a, b)]
//! fn a(dummy: &mut Dummy) {
//!   dummy.a = 0;
//! }
//! ```
//! When expanded, the above would replace all `a` tokens with `b` tokens to
//! produce
//! ```
//! # struct Dummy { a: usize, b: usize }
//! # impl Dummy { fn new() -> Self { Self { a: 0, b: 1 } } }
//! fn b(dummy: &mut Dummy) {
//!   dummy.b = 0;
//! }
//! ```
//! 
//! On its own, this isn't very useful. The intended use-case for this macro
//! is in combination with external attribute proc macros. The example below
//! is a concrete use case
//! 
//! # Example
//! Suppose that we want to expose a macro that makes use of [linkme]'s 
//! distributed slice functionality. We could do something like this
//! ```
//! #[doc(hidden)]
//! pub mod exports {
//!   pub extern crate linkme;
//! 
//!   #[linkme::distributed_slice]
//!   pub static SLICE: [u8] = [..];
//! }
//! 
//! #[macro_export]
//! macro_rules! declare {
//!   { $vis:vis static $name:ident : $ty:ty = $value:expr; } => {
//!     #[$crate::export::linkme::distributed_slice($crate::export::SLICE)]
//!     $vis static $name : $ty = $value;
//!   }
//! }
//! ```
//! There's a problem here. The [linkme] crate expects that it can access
//! its exported items under the `linkme::*` path. In downstream crates that
//! won't necessarily be true. We could require that every crate that wants
//! to use this macro import linkme manually, but that's not very convenient.
//! Instead, we can use the [`replace`] macro to replace `linkme` with a path
//! pointing back to our exported crate instead. If we do this, we get
//! ```
//! #[doc(hidden)]
//! pub mod exports {
//!   pub extern crate linkme;
//!   pub use tokenreplace::replace;
//! 
//!   #[linkme::distributed_slice]
//!   pub static SLICE: [u8] = [..];
//! }
//! 
//! #[macro_export]
//! macro_rules! declare {
//!   { $vis:vis static $name:ident : $ty:ty = $value:expr; } => {
//!     #[$crate::export::linkme::distributed_slice($crate::export::SLICE)]
//!     #[$crate::export::replace(linkme, example::export::linkme)] // <<
//!     $vis static $name : $ty = $value;
//!   }
//! }
//! ```
//! Since the linkme crate correctly propagates internal attributes onto the
//! final static declaration, the replace macro is passed through and replaces
//! all instances of `linkme` in the expanded code with 
//! `example::export::linkme`, allowing downstream crates to use the new crate
//! without having to import linkme manually.
//! 
//! Unfortunately, trying to put `$crate` in the replace path doesn't work but
//! this can be worked around by just using the literal crate name.
//! 
//! [`replace`]: macro@crate::replace
//! [linkme]: https://docs.rs/linkme

extern crate proc_macro;

use proc_macro2::{Group, TokenStream, TokenTree};
use quote::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::Token;

#[allow(dead_code)]
struct Meta {
  search: syn::Ident,
  comma: Token![,],
  replace: TokenStream,
}

impl Parse for Meta {
  fn parse(input: ParseStream) -> syn::parse::Result<Self> {
    Ok(Meta {
      search: input.parse()?,
      comma: input.parse()?,
      replace: input.parse()?,
    })
  }
}

fn walk_stream(meta: &Meta, stream: TokenStream, out: &mut TokenStream) {
  for tree in stream {
    walk_tree(meta, tree, out);
  }
}

fn walk_tree(meta: &Meta, tree: TokenTree, out: &mut TokenStream) {
  match tree {
    TokenTree::Group(group) => {
      let mut stream = TokenStream::new();
      walk_stream(meta, group.stream(), &mut stream);
      Group::new(group.delimiter(), stream).to_tokens(out);
    }
    TokenTree::Ident(ident) if ident.to_string() == meta.search.to_string() => {
      meta.replace.to_tokens(out);
    }
    x => x.to_tokens(out),
  }
}

fn token_replace(attr: TokenStream, input: TokenStream) -> TokenStream {
  let meta: Meta = match syn::parse(attr.into()) {
    Ok(meta) => meta,
    Err(e) => return e.to_compile_error()
  };

  let mut output = TokenStream::new();
  walk_stream(&meta, input, &mut output);

  return output;
}

#[proc_macro_attribute]
pub fn replace(attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
  token_replace(attr.into(), item.into()).into()
}

