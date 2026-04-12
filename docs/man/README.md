# Man pages

This project uses [scdoc](https://git.sr.ht/~sircmpwn/scdoc) for manual pages.

`scdoc` reads scdoc markup from standard input and writes roff to standard
output.

## Build

From the repo root, use `scripts` itself:

```sh
scripts run man
```

Clean generated manpages with:

```sh
scripts run clean-man
```

Or build files directly:

```sh
mkdir -p target/man
scdoc < docs/man/scripts.1.scd > target/man/scripts.1
scdoc < docs/man/SCRIPTS.5.scd > target/man/SCRIPTS.5
```

## Preview

```sh
man ./target/man/scripts.1
man ./target/man/SCRIPTS.5
```
