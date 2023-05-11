# Building

## Docker image

Who uses docker anyway?

## Building from source

### client

To build the niketsu client from source, clone the repo:

```bash
git clone https://github.com/sevenautumns/niketsu.git
cd niketsu
```

And use the build tools of Rust:

```bash
cargo build --release
./niketsu-server
```

### server

To build the niketsu server from source, clone the repo:

```bash
git clone https://github.com/sevenautumns/niketsu.git
cd niketsu
```

And use the build tools of Go:

```bash
go build -o niketsu-server server/main.go
./niketsu-server
```

## nix

### Development

Install the almighty [Nix](https://nixos.wiki/wiki/Nix_Installation_Guide), clone the repository and run:

```bash
nix develop
```

All dependencies for the client and the server are now included in the dev-shell.


### Building

In addition, client and server can be built with the the `flake.nix` file in the root directory of the repository.

#### client

```bash
nix build .#niketsu-client
```
#### server

```bash
nix build .#niketsu-server
```
