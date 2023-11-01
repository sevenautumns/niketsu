# Build Like a Pro: Your Comprehensive Guide to niketsu Deployment 🛠️

## Unleash Docker Magic 🐳

Too cool for Docker? Think again. We offer a sleek, ready-to-go Dockerfile for the server side, ensuring you're up and running in no time.

👇 **One-Click Build**:
```bash
podman build -t niketsu-server:latest .
```
👇 **Effortless Deployment**:
```bash
podman run -p 7766:7666 niketsu-server:latest
```

For the Docker aficionados, you can even customize your setup with environment variables. Dive into our [Usage Page](usage.md#Arguments) for more details.

## Hand-Craft Your Source Build 🚀

### For the Client

Join the cutting edge—build the niketsu client from source.
  
1. **Clone & Navigate**
    ```bash
    git clone https://github.com/sevenautumns/niketsu.git
    cd niketsu
    ```
2. **Compile & Run**
    ```bash
    cargo build --release
    ./target/release/niketsu
    ```

### For the Server

Want more control? Build your own niketsu server from the ground up.
  
1. **Clone & Navigate**
    ```bash
    git clone https://github.com/sevenautumns/niketsu.git
    cd niketsu
    ```
2. **Compile & Run**
    ```bash
    go build -o niketsu-server server/main.go
    ./niketsu-server
    ```

## The Nix Nirvana 🌀

Already a Nix enthusiast? We've got you covered.

### Effortless Development

1. Install [Nix](https://nixos.wiki/wiki/Nix_Installation_Guide).
2. Clone the repository.
3. **Enter Dev-Mode**: `nix develop`
  
All you need, conveniently packed into one shell.

#### Running the Beast

- **Server**: `go run server/main.go`
- **Client**: `cargo run --release`

### The Nix Build

Want even more control? Utilize our `flake.nix` to tailor your build.

- **Client**: `nix build .#niketsu-client`
- **Server**: `nix build .#niketsu-server`

**Whatever your tech stack, niketsu seamlessly integrates, empowering you to create the ultimate shared viewing experience. Build it your way, run it your way.** 🌠
