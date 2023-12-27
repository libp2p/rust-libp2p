use syn::{Expr, ExprLit, Lit};

pub(crate) trait RequireStrLit {
    fn require_str_lit(&self) -> syn::Result<String>;
}

impl RequireStrLit for Expr {
    fn require_str_lit(&self) -> syn::Result<String> {
        match self {
            Expr::Lit(ExprLit {
                lit: Lit::Str(str), ..
            }) => Ok(str.value()),
            _ => Err(syn::Error::new_spanned(self, "expected a string literal")),
        }
    }
}
