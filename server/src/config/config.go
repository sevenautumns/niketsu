package config

import (
	"encoding/json"
	"log"

	"github.com/alecthomas/kong"
)

const (
	niketsuGlobalPath  = "/etc/niketsu.json"
	niketsuLocalPath   = "~/.config/niketsu.json"
	niketsuProjectPath = "./niketsu.json"
)

type CLI struct {
	Config           kong.ConfigFlag `name:"config" env:"CONFIG" help:"path to a custom config file" json:"config"`
	Host             string          `name:"host" default:"" env:"HOST" help:"host name (e.g. 0.0.0.0). If left empty (= ''), listens on all IPs of the machine" json:"host"`
	Port             uint16          `name:"port" default:"7766" env:"PORT" help:"port (range from 0 to 65535) to listen on" json:"port"`
	Cert             string          `name:"cert" default:"" env:"CERT" help:"path to TLS certificate file. If none is given, plain TCP is used" json:"cert"`
	Key              string          `name:"key" default:"" env:"KEY" help:"path to TLS key corresponding to the TLS certificate. If none is given, plain TCP is used" json:"key"`
	Password         string          `name:"password" default:"" env:"PASSWORD" help:"general server password for client connections" json:"password"`
	DBPath           string          `name:"dbpath" default:"./.db/" env:"DBPATH" help:"path to where database files are stored" json:"dbpath"`
	DBUpdateInterval uint64          `name:"dbupdateinterval" default:"10" env:"DBUPDATEINTERVAL" help:"update intervals (in seconds) of writes to the database" json:"dbupdateinterval"`
	DBWaitTimeout    uint64          `name:"dbwaittimeout" default:"4" env:"DBWAITTIMEOUT" help:"wait time (in seconds) until write to database is aborted" json:"dbwaittimeout"`
	Debug            bool            `name:"debug" env:"DEBUG" help:"whether to log debugging entries" json:"debug"`
}

// Parses command arguments, environment variables and config file in case one is given.
// Order of precedence is: environment variables < config file < command arguments
func ParseCommandArgs() CLI {
	var cli CLI
	kong.Parse(&cli,
		kong.Name("niketsu server"),
		kong.Description("Run the niketsu video player synchronization server"),
		kong.UsageOnError(),
		kong.ConfigureHelp(kong.HelpOptions{
			Compact: true,
			Summary: false,
		}),
		kong.Configuration(kong.JSON, niketsuGlobalPath, niketsuLocalPath, niketsuProjectPath),
	)

	return cli
}

func PrintConfig(cli CLI) {
	s, _ := json.MarshalIndent(cli, "", "\t")
	log.Printf("Configurations successfully set:\n%s", string(s))
}
