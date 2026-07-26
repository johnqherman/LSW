# Examples

Complete projects you can copy or run in place.

- [`cmake-hello`](cmake-hello/) - the smallest real LSW project: a C console
  program built with CMake, a ctest test that runs under Wine, an `[env.vars]`
  entry, and a `win10` API floor.

Run one:

```
cd examples/cmake-hello
lsw setup
lsw check
```

`lsw.lock` is gitignored here because these examples float with the LSW
version; in your own project, commit both `lsw.toml` and `lsw.lock`.
