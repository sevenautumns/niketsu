# Build Like a Pro: Your Comprehensive Guide to niketsu Deployment 🛠️

<!--
## Unleash Docker Magic 🐳

Too cool for Docker? Think again. We offer a sleek, ready-to-go Dockerfile for the intermediate relay, ensuring you're up and running in no time if you do not feel like using our self-hosted relay services.

👇 **One-Click Build**:
```bash
podman build -t niketsu-server:latest .
```
👇 **Effortless Deployment**:
```bash
podman run -p 7766:7666 niketsu-server:latest
```

For the Docker aficionados, you can even customize your setup with environment variables. Dive into our [Usage Page](usage.md#Arguments) for more details.
-->

## Requirements

Running the niketsu-client requires an installation of [mpv](https://mpv.io/) (preferably the latest version).

## Hand-Craft Your Source Build 🚀

Join the cutting edge — build niketsu from source.
  
1. **Clone & Navigate**
    ```bash
    git clone https://github.com/sevenautumns/niketsu.git
    cd niketsu
    ```

2. **Compile**
    ```bash
    # All
    cargo build --release
    # Client only
    cargo build --release --bin niketsu
    # Relay only
    cargo build --release --bin niketsu-relay
    ```

3. **Run**
    ```bash
    ./target/release/niketsu
    ```

</br>

Want more control? Run your own niketsu relay after compiling using the instructions above:

```bash
./target/release/niketsu-relay
```

Check out the help page on the command line by using `--help` or visit the [usage web page](http://localhost:3000/usage.html).

## The Nix Nirvana 🌀

Already a Nix enthusiast? We've got you covered.

### Effortless Development

1. Install [Nix](https://nixos.wiki/wiki/Nix_Installation_Guide).
2. Clone the repository.
3. **Enter Dev-Mode**: `nix develop`
  
All you need, conveniently packed into one shell.

#### Running the Beast

- **Relay-Server**: `cargo run --release --bin niketsu-relay`
- **Client**: `cargo run --release --bin niketsu`

#### Examples for Debugging (Client only)

`cargo run --example [OPTION]`

### The Nix Build

Want even more control? Utilize our `flake.nix` to tailor your build.

- **Relay-Server**: `nix build .#niketsu-relay`
- **Client**: `nix build .#niketsu-client`

<br>
<hr>

**Whatever your tech stack, niketsu seamlessly integrates, empowering you to create the ultimate shared viewing experience. Build it your way, run it your way.** 🌠
