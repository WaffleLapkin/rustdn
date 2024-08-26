//! Module with std-like things, in other words "utilities".

pub trait AnyExt: Sized {
    fn apply<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }

    fn also(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }
}

impl<T> AnyExt for T {}

/// Destructures `$e` using a provided pattern.
///
/// Importantly, this works with types which implement `Drop` (ofc, this doesn't run the destructor).
// FIXME: move this to its own crate
#[macro_export]
macro_rules! destructure {
    ($Type:ident { $($f:tt $(: $rename:pat)? ),+ $(,)? } = $e:expr) => (
        // FIXME: use $crate:: paths
        let tmp = $crate::unstd::_macro_reexport::core::mem::ManuallyDrop::new($e);

        // assert that `$e` is an owned expression, rather than `&Type`
        if false {
            #[allow(unreachable_code)]
            let _assert_owned_expr = [&tmp, &$crate::unstd::_macro_reexport::core::mem::ManuallyDrop::new($Type { $($f: todo!()),* })];
        };

        $(
            let $crate::destructure!(@_internal_pat_helper $f $($rename)?)
                // safety: `$e` is of type `$Type<..>` (as asserted above),
                //         so we have ownership over it's fields.
                //         `$f` is a field of `$Type<..>` (as asserted above).
                //         `$e` is moved into a `ManuallyDrop`, which means its `drop` won't be run,
                //         so we can safely move out its
                //         the pointer is created from a reference and is thus valid
                = unsafe { $crate::unstd::_macro_reexport::core::ptr::read(&tmp.$f) };
        )+

        // remove the temporary we don't need anymore.
        // doesn't actually drop, since `ManuallyDrop`.
        _ = {tmp};
    );
    (@_internal_pat_helper $f:tt) => ($f);
    (@_internal_pat_helper $f:tt $rename:pat) => ($rename);
}

#[doc(hidden)]
#[allow(unused_imports)]
pub mod _macro_reexport {
    pub use core;
}
