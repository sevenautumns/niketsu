package main

import (
	"context"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"syscall"

	"github.com/sevenautumns/niketsu/relay-go/internal/config"
	"github.com/sevenautumns/niketsu/relay-go/internal/logging"
	"github.com/sevenautumns/niketsu/relay-go/internal/relay"
)

func main() {
	var (
		portFlag  = flag.Uint("port", 0, "Override config port for this run (alias -p)")
		portShort = flag.Uint("p", 0, "Override config port for this run")
		levelFlag = flag.String("log-level", "off", "off|error|warn|info|debug|trace (alias -t)")
		levelShrt = flag.String("t", "off", "off|error|warn|info|debug|trace")
	)
	flag.Parse()

	cfg := config.LoadOrDefault()
	configPath, _ := config.ConfigPath()
	switch {
	case *portFlag != 0:
		cfg.Port = uint16(*portFlag)
	case *portShort != 0:
		cfg.Port = uint16(*portShort)
	}
	level := logging.LevelOff
	if *levelFlag != "off" {
		level = logging.ParseLevel(*levelFlag)
	} else if *levelShrt != "off" {
		level = logging.ParseLevel(*levelShrt)
	}

	logPath, _ := config.CachePath()
	closer := logging.Setup(level, logPath)
	defer closer.Close()

	r, updatedCfg, err := relay.New(cfg, configPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "fatal: %v\n", err)
		os.Exit(1)
	}

	if updatedCfg != nil && updatedCfg.Keypair != cfg.Keypair {
		if err := updatedCfg.Save(); err != nil {
			slog.Warn("failed to save config", "err", err)
		}
	}

	slog.Info("relay started",
		"peer_id", r.PeerID(),
		"addrs", r.ListenAddrs(),
		"persistence", cfg.Persistence.Enabled,
		"metrics", cfg.Metrics.Enabled,
	)

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	if err := r.Run(ctx); err != nil {
		fmt.Fprintf(os.Stderr, "shutdown error: %v\n", err)
		os.Exit(1)
	}
	slog.Info("relay stopped cleanly")
}
