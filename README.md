# resolvo-rpm

This project downloads repodata from a yum/dnf/rpm repository and tries to resolve for the packages the user requests.
It uses the [`resolvo`](https://github.com/mamba-org/resolvo) crate to do the actual SAT resolution (instead of `libsolv`).

## Usage

```
$ cargo run --release -- curl

Resolved:

- basesystem=11
- bash=5.2.15
- ca-certificates=2023.2.60
- coreutils-common=9.1
- coreutils=9.1
- curl=7.87.0
...
```

### Shortcomings

This is currently just an initial proof of concept. We're hoping that the community will help us figure some things out.

- [ ] Handle `suggests` properly. Currently can only be disabled globally with `--disable-suggests` but should be iteratively removed if they conflict with other packages.
- [ ] Handle `recommends` properly. Not handled at all.
- [ ] Handle `conflicts` properly. This should be fairly straightforward as we have this capability in resolvo.
- [ ] Handle `obsoletes` properly. Not handled at all.
- [ ] Try to actually install packages using `rpm-rs`.
- [ ] Store repodata in a better cache or load only what we need as repodata loading is somewhat slow right now.

There is also this open issue to track some of the ideas in `resolvo`: https://github.com/mamba-org/resolvo/issues/1