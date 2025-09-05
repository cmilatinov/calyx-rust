#[macro_export]
macro_rules! try_all {
    (
        None => $on_none:stmt;
        $(
            let $pat:pat = $expr:expr;
        )+
    ) => {
        $(
            let Some($pat) = $expr else {
                $on_none
            };
        )+
    };
}

#[macro_export]
macro_rules! try_some_continue {
    ($expr:expr) => {
        match $expr {
            Some(val) => val,
            _ => continue,
        }
    };
}

#[macro_export]
macro_rules! try_some_return {
    ($expr:expr) => {
        match $expr {
            Some(val) => val,
            _ => {
                return;
            }
        }
    };
}
