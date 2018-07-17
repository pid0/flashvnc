flashvnc
===============
flashvnc is yet another VNC client.
Its design goals are compatibility with the TurboVNC server as well as good performance when run over a high-bandwidth connection.
It accomplishes this by aggressively parallelizing the decoding and rendering processes, unlike any other VNC clients as far as I know, including the TurboVNC client at the time of this writing.
As a result, it is typically much faster, up to the theoretical speed-up depending on processor cores, than the TurboVNC client on multi-core but otherwise weak client machines.

Although still a work-in-progress project, it does support all mandatory RFB features as well as all extensions required for a session with the TurboVNC server.
It does not support any kind of authentication.

Other nice things
--------------------
flashvnc is written in Rust with a test-first approach. It has automated GUI tests.

flashvnc defines almost all of the RFB protocol declaratively in a domain-specific language. The DSL uses a very thin layer of Rust macro magic on top of a small parser combinator library. For example, the Screen packet of RFB is declared with:

```Rust
packet! { Screen:
    [id : [u32_be()] -> u32]
    [x : [u16_be()] -> u16]
    [y : [u16_be()] -> u16]
    [width : [length(u16_be())] -> usize]
    [height : [length(u16_be())] -> usize]
    [flags : [u32_be()] -> u32]
}
```

This defines a class `Screen` that can be used to parse as well as to synthesize Screen packets.

Build
========
Standard `cargo` build. Use the nightly release of rustc.

The `end_to_end_spec` must be run with `--test-threads=1`. It is easiest to run it directly from the target directory.

The unit tests can be run with `cargo test --lib`.

Documentation
==============
WIP

License
==============
GPLv3+. See LICENSE file.

Author
===============
Patrick Plagwitz <patrick_plagwitz@web.de>.
