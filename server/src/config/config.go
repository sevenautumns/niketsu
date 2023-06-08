package config

import (
	"encoding/json"
	"log"

	"github.com/BurntSushi/toml"
	"github.com/jessevdk/go-flags"
)

type RoomConfig struct {
	Persistent bool
}

type GeneralConfig struct {
	ConfigPath       string `long:"config" default:"" env:"CONFIG" description:"path to config file (toml)"`
	Host             string `long:"host" default:"" env:"HOST" description:"host name (e.g. 0.0.0.0). If left empty (= ''), listens on all IPs of the machine"`
	Port             uint16 `long:"port" default:"7766" env:"PORT" description:"port (range from 0 to 65535) to listen on"`
	Cert             string `long:"cert" default:"" env:"CERT" description:"path to TLS certificate file. If none is given, plain TCP is used"`
	Key              string `long:"key" default:"" env:"KEY" description:"path to TLS key corresponding to the TLS certificate. If none is given, plain TCP is used"`
	Password         string `long:"password" default:"" env:"PASSWORD" description:"general server password for client connections"`
	DBPath           string `long:"dbpath" default:"./.db/" env:"DBPATH" description:"path to where database files are stored"`
	DBUpdateInterval uint64 `long:"dbupdateinterval" default:"10" env:"DBUPDATEINTERVAL" description:"update intervals (in seconds) of writes to the database"`
	DBWaitTimeout    uint64 `long:"dbwaittimeout" default:"4" env:"DBWAITTIMEOUT" description:"wait time (in seconds) until write to database is aborted"`
	Debug            bool   `long:"debug" env:"DEBUG" description:"whether to log debugging entries"`
}

type Config struct {
	General GeneralConfig
	Rooms   map[string]RoomConfig
}

// Parses command arguments, environment variables and config file in case one is given.
// Order of precedence is: config file < environment variables < command arguments
func GetConfig() Config {
	generalConfig := parseCommandArgs()

	config := Config{Rooms: make(map[string]RoomConfig, 0)}
	if generalConfig.ConfigPath != "" {
		readConfigFile(generalConfig.ConfigPath, &config)
		mergeConfigs(generalConfig, &config)
	} else {
		config.General = generalConfig
	}
	printConfig(config)

	return config
}

func parseCommandArgs() GeneralConfig {
	var general GeneralConfig
	parser := flags.NewParser(&general, 0)
	parser.Parse()

	return general
}

func readConfigFile(path string, config *Config) {
	_, err := toml.DecodeFile(path, &config)
	if err != nil {
		log.Fatalf("Failed to load config file. Given: %s. Make sure the correct file format (toml) is used and the file exists.\nError:%s", path, err)
	}
}

func mergeConfigs(generalConfig GeneralConfig, config *Config) {
	enc, err := json.Marshal(generalConfig)
	if err != nil {
		log.Fatalf("Failed to marshal configuration. Error: %s", err)
	}

	err = json.Unmarshal(enc, &config.General)
	if err != nil {
		log.Fatalf("Failed to umarshal configuration. Error: %s", err)
	}
}

func printConfig(serverConfig Config) {
	s, _ := json.MarshalIndent(serverConfig, "", "\t")
	log.Printf("Configurations successfully set:\n%s", string(s))
}
