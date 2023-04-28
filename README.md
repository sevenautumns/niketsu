<h1 align="center" style="border-bottom: none;">
 niketsu
</h1>

<p align="center">Refer to the owner of the repository for the full documentation, examples and guides *wink wink*</p>

<div align="center">

![[Build Action Card](https://github.com/sevenautumns/niketsu/actions/workflows/build.yml/badge.svg)](https://github.com/sevenautumns/niketsu/actions/workflows/build.yml/badge.svg)
![[Check Action Card](https://github.com/sevenautumns/niketsu/actions/workflows/check.yaml/badge.svg)](https://github.com/sevenautumns/niketsu/actions/workflows/check.yaml/badge.svg)
![[Nightly Tag Card](https://github.com/sevenautumns/niketsu/actions/workflows/tag.yaml/badge.svg)](https://github.com/sevenautumns/niketsu/actions/workflows/tag.yaml/badge.svg)
[![Rust Report Card](https://rust-reportcard.xuri.me/badge/github.com/sevenautumns/niketsu)](https://rust-reportcard.xuri.me/report/github.com/sevenautumns/niketsu)
[![Go Report Card](https://goreportcard.com/badge/github.com/sevenautumns/niketsu)](https://goreportcard.com/report/github.com/sevenautumns/niketsu)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://github.com/sevenautumns/niketsu/blob/main/LICENSE)

</div>

Naive video synchronization between multiple clients of mpv.

## Install

There are various ways of installing niketsu.

### Precompiled binaries

Precompiled binaries for released versions are available in the [*releases* section](https://github.com/sevenautumns/niketsu/releases). Using the latest production release binary is the recommended way of installing niketsu. Since the releases are still not stable, who knows if the maintainers make updates backward compatible. My best guess is, updates will break everything *wink wink*.
Make sure to install the respective client and server versions.

### Docker images

Who uses docker anyway?

### Building from source


### nix

machste-nix

#### client

To build the niketsu client from source, you need:

#### server

To build the niketsu server from source, clone the repo:

```bash
git clone https://github.com/sevenautumns/niketsu.git
cd niketsu
```

You can use the `go` tool to build and install the `niketsu` binary into your `GOPATH`:

```bash
go install github.com/sevenautumns/niketsu/server
server --config=server/config.toml
```

or alternatively using `go build`:

```bash
go build -o niketsu-server server/main.go
./niketsu-server --config=server/config.toml
```

## More information

No

### Not frequently asked questions

#### Why use niketsu?

I do not know, honestly.

#### What are some features of niketsu?

* It is blazingly lightweight. Due to its lack of consistency, everything is somewhat lightweight even if it is not.
* It is written in Rust and Go. Therefore, unprecedented levels of synergy can be reached. More diversity, more pantyhose, more fun.

* Compared to rival products, it actually works with network mounts and is completely async. No more freezing of the main window during winter.
* There is more Readwritelocks than necessary.
* The code is still somewhat simple.

##### 

## Contributing

Slide into the DMs of the owner of the repository *wink wink*.


## License

Apache License 2.0, see [LICENSE](https://github.com/sevenautumns/niketsu/blob/main/LICENSE-APACHE).

