#[cfg(test)]
pub mod common;
#[cfg(test)]
pub mod net_server;
#[cfg(test)]
pub mod acid;
#[cfg(test)]
pub mod concurrency;
#[cfg(test)]
pub mod sql_conformance;
#[cfg(test)]
pub mod tutorial_validation;
#[cfg(test)]
pub mod integration;

/// Generates two additional test variants for a backend-parameterised test
/// function: one connecting to the plain-TCP network server and one to the
/// TLS network server.
///
/// Usage:
/// ```rust,ignore
/// fn test_foo_body(b: &crate::common::Backend) { /* body */ }
///
/// #[test]
/// fn test_foo() {
///     let dir = tempfile::TempDir::new().unwrap();
///     let b = crate::common::Backend::embedded(dir.path());
///     test_foo_body(&b);
/// }
///
/// crate::net_tests!(test_foo);
/// ```
///
/// This expands to:
///   - `test_foo_net`     — runs `test_foo_body` against plain TCP
///   - `test_foo_net_tls` — runs `test_foo_body` against TLS
#[macro_export]
macro_rules! net_tests {
    ($fn_name:ident) => {
        paste::paste! {
            #[test]
            #[serial_test::serial(network_plain)]
            fn [<$fn_name _net>]() {
                let b = $crate::net_server::plain_backend(stringify!($fn_name));
                [<$fn_name _body>](&b);
            }

            #[test]
            #[serial_test::serial(network_tls)]
            fn [<$fn_name _net_tls>]() {
                let b = $crate::net_server::tls_backend(stringify!($fn_name));
                [<$fn_name _body>](&b);
            }
        }
    };
}
