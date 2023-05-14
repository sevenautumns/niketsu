package niketsu_server

import (
	"encoding/json"
	"log"

	"github.com/BurntSushi/toml"
	"github.com/jessevdk/go-flags"
)

type RoomConfig struct {
	Persistent bool
}

type General struct {
	Config           string `long:"config" default:"" env:"CONFIG" description:"path to config file (toml)"`
	Host             string `long:"host" default:"" env:"HOST" description:"host name (e.g. 0.0.0.0). If left empty (= ''), listens on all IPs of the machine"`
	Port             uint16 `long:"port" default:"7766" env:"PORT" description:"port (range from 0 to 65535) to listen on"`
	Cert             string `long:"cert" default:"" env:"CERT" description:"path to TLS certificate file. If none is given, plain TCP is used"`
	Key              string `long:"key" default:"" env:"KEY" description:"path to TLS key corresponding to the TLS certificate. If none is given, plain TCP is used"`
	Password         string `long:"password" default:"" env:"PASSWORD" description:"general server password for client connections"`
	DBPath           string `long:"dbpath" default:"./.db/" env:"DBPATH" description:"path to where database files are stored"`
	DBUpdateInterval uint64 `long:"dbupdateinterval" default:"2" env:"DBUPDATEINTERVAL" description:"update intervals (in seconds) of writes to the database"`
	DBWaitTimeout    uint64 `long:"dbwaittimeout" default:"4" env:"DBWAITTIMEOUT" description:"wait time (in seconds) until write to database is aborted"`
	DBStatInterval   uint64 `long:"dbstatinterval" default:"120" env:"DBSTATINTERVAL" description:"update intervals (in seconds) of logged database statistics"`
	Debug            bool   `long:"debug" env:"DEBUG" description:"whether to log debugging entries"`
}

type ServerConfig struct {
	General General
	Rooms   map[string]RoomConfig
}

func fromConfigFile(general General, serverConfig *ServerConfig) {
	_, err := toml.DecodeFile(general.Config, &serverConfig)
	if err != nil {
		log.Panicf("Failed to load config file. Given: %s. Make sure the correct file format (toml) is used and the file exists.\nError:%s", general.Config, err)
	}

	enc, err := json.Marshal(general)
	if err != nil {
		log.Panicf("Failed to marshal configuration. Error: %s", err)
	}

	err = json.Unmarshal(enc, &serverConfig.General)
	if err != nil {
		log.Panicf("Failed to umarshal configuration. Error: %s", err)
	}
}

func printConfig(serverConfig ServerConfig) {
	s, _ := json.MarshalIndent(serverConfig, "", "\t")
	log.Printf("Configurations successfully set:\n%s", string(s))
}

func GetConfig() ServerConfig {
	var general General
	parser := flags.NewParser(&general, 0)
	parser.Parse()

	serverConfig := ServerConfig{Rooms: make(map[string]RoomConfig, 0)}
	if general.Config != "" {
		fromConfigFile(general, &serverConfig)
	} else {
		serverConfig.General = general
	}

	printConfig(serverConfig)
	return serverConfig
}
