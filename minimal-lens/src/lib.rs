pub trait Has<T> {
    fn extract(self) -> T;
}

pub trait HasRef<T> {
    fn extract_ref(&self) -> &T;
}

pub trait HasRefMut<T> {
    fn extract_ref_mut(&mut self) -> &mut T;
}

impl<'a, T, Out> Has<&'a Out> for &'a T
where
    T: HasRef<Out>,
{
    fn extract(self) -> &'a Out {
        self.extract_ref()
    }
}

impl<'a, T, Out> Has<&'a mut Out> for &'a mut T
where
    T: HasRefMut<Out>,
{
    fn extract(self) -> &'a mut Out {
        self.extract_ref_mut()
    }
}

pub trait HasExt {
    fn get_owned<T>(self) -> T
    where
        Self: Has<T>;
    fn get<'a, T>(&'a self) -> &'a T
    where
        &'a Self: Has<&'a T>;
    fn get_mut<'a, T>(&'a mut self) -> &'a mut T
    where
        &'a mut Self: Has<&'a mut T>;
}

impl<Container> HasExt for Container {
    fn get_owned<T>(self) -> T
    where
        Self: Has<T>,
    {
        Has::extract(self)
    }

    fn get<'a, T>(&'a self) -> &'a T
    where
        &'a Self: Has<&'a T>,
    {
        Has::extract(self)
    }

    fn get_mut<'a, T>(&'a mut self) -> &'a mut T
    where
        &'a mut Self: Has<&'a mut T>,
    {
        Has::extract(self)
    }
}

#[macro_export]
macro_rules! impl_has_owned {
    ($self_ty:ty, $typ:ty, |$arg_name:pat| $get_expr:expr) => {
        impl Has<$typ> for $self_ty {
            fn extract(self) -> $typ {
                let $arg_name = self;
                $get_expr
            }
        }
    }
}

#[macro_export]
macro_rules! impl_has_ref {
    ($self_ty:ty, $typ:ty, |$arg_name:pat| $get_expr:expr) => {
        impl HasRef<$typ> for $self_ty {
            fn extract_ref(&self) -> &$typ {
                let $arg_name = self;
                $get_expr
            }
        }
    }
}

#[macro_export]
macro_rules! impl_has_mut {
    ($self_ty:ty, $typ:ty, |$arg_name:pat| $get_expr:expr) => {
        impl HasRefMut<$typ> for $self_ty {
            fn extract_ref_mut(&mut self) -> &mut $typ {
                let $arg_name = self;
                $get_expr
            }
        }
    }
}

#[macro_export]
macro_rules! impl_has {
    ($self_ty:ty, $typ:ty, |$arg_name:pat| $get_expr:expr) => {
        impl_has_owned!($self_ty, $typ, |$arg_name| $get_expr);
        impl_has_ref!($self_ty, $typ, |$arg_name| &$get_expr);
        impl_has_mut!($self_ty, $typ, |$arg_name| &mut $get_expr);
    }
}

#[cfg(test)]
mod tests {
    use super::{Has, HasExt, HasRef, HasRefMut};

    #[derive(PartialEq, Debug)]
    struct Foo;

    struct Bar(Foo);

    impl_has!(Bar, Foo, |bar| bar.0);

    #[test]
    fn can_get_all() {
        let mut bar = Bar(Foo);

        {
            let foo: &Foo = bar.get::<Foo>();

            assert_eq!(foo, &Foo);
        }

        {
            let foo: &mut Foo = bar.get_mut::<Foo>();

            assert_eq!(foo, &Foo);
        }

        let foo_owned = bar.get_owned::<Foo>();

        assert_eq!(foo_owned, Foo);
    }

    #[test]
    fn function_bounds() {
        fn takes_get_ref<T: HasRef<Foo>>(val: &T) -> &Foo {
            val.get()
        }

        fn takes_get_ref_mut<T: HasRefMut<Foo>>(val: &mut T) -> &mut Foo {
            val.get_mut()
        }

        let mut bar = Bar(Foo);

        takes_get_ref(&bar);
        takes_get_ref_mut(&mut bar);
    }
}
