package niketsu_server

import (
	"flag"
	"log"

	"github.com/BurntSushi/toml"
)

type RoomConfig struct {
	persistent bool
}

type General struct {
	Host             string
	Port             uint16
	Cert             string
	Key              string
	Password         string
	DBPath           string
	DbUpdateInterval uint64
	DbWaitTimeout    uint64
	DbStatInterval   uint64
	Debug            bool
}

type ServerConfig struct {
	General General
	Rooms   map[string]RoomConfig
}

func fromConfigFile(configFile string, serverConfig *ServerConfig) ServerConfig {
	_, err := toml.DecodeFile(configFile, &serverConfig)
	if err != nil {
		log.Panicf("Failed to load config file at %s. Make sure the correct file format (toml) is used and the file exists.\nError:%s", configFile, err)
	}
	log.Printf("Configurations successfully set.\nServer configuration: %v", serverConfig)

	return *serverConfig
}

func GetConfig() ServerConfig {
	configFile := flag.String("config", "", "path to config file (toml)")
	host := flag.String("host", "", "host name (e.g. 0.0.0.0). If left empty (= ''), listens on all IPs of the machine")
	port := flag.Uint("port", 7766, "port (16bit unsigned integer) to listen on")
	cert := flag.String("cert", "", "TLS certificate. If none is given, plain TCP is used")
	key := flag.String("key", "", "TLS key corresponding to the TLS certificate. If none is given, plain TCP is used")
	password := flag.String("password", "", "general server password for client connections")
	dbPath := flag.String("dbpath", ".db/", "path to where database files are stored")
	dbUpdateInterval := flag.Uint64("dbupdateinterval", 2, "update intervals of writes to the database")
	dbWaitTimeout := flag.Uint64("dbwaittimeout", 4, "wait time until write to database is aborted")
	dbStatInterval := flag.Uint64("dbstatinterval", 120, "update intervals of database statistics")
	debug := flag.Bool("debug", false, "whether to log debugging entries")

	flag.Parse()

	var serverConfig ServerConfig
	serverConfig.General = General{Host: *host, Port: uint16(*port), Cert: *cert, Key: *key, Password: *password, DBPath: *dbPath, DbUpdateInterval: *dbUpdateInterval, DbWaitTimeout: *dbWaitTimeout, DbStatInterval: *dbStatInterval, Debug: *debug}

	log.Printf(*configFile)
	if *configFile != "" {
		return fromConfigFile(*configFile, &serverConfig)
	}

	return serverConfig
}
