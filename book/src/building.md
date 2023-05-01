# Building

## Docker images

Who uses docker anyway?

## Building from source

### client

In progress

### server

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

## Development

### nix

Install the almighty [Nix](https://nixos.wiki/wiki/Nix_Installation_Guide), clone the repository and run:

```bash
nix develop
```

Honestly, it is that simple (as of yet, some Go dependencies might not work in the development shell).
