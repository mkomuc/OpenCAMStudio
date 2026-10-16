# block 0.1.6 compatibility patch

Source: the crates.io `block` 0.1.6 release, by Steven Sheldon, licensed MIT.
Upstream: <https://github.com/SSheldon/rust-block>.

The macOS renderer uses this crate through `wgpu-hal` and `metal`. Its
`_NSConcreteStackBlock` foreign static was declared with an uninhabited empty
enum. Rust reports this as a future incompatibility (rust-lang/rust#74840).

The patch replaces that private type with `c_void`, preserving the opaque
pointer's ABI and the runtime symbol's address. `addr_of!` takes the address
without forming a Rust reference to the opaque foreign data. The patch also
spells out the existing implicit C ABI as `extern "C"` to satisfy current Rust's
`missing_abi` lint.
No lint is disabled. The manifest drops the unpublished `test_utils` path
override; upstream's test helper remains a registry dev dependency.

Remove this patch when the renderer stops using `block` 0.1.6 or an upstream
release fixes the declaration.
