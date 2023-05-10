package niketsu_server

import (
	"encoding/json"
	"flag"
	"log"

	"github.com/BurntSushi/toml"
)

type RoomConfig struct {
	Persistent bool
}

type General struct {
	Host             *string
	Port             *uint16
	Cert             *string
	Key              *string
	Password         *string
	DBPath           *string
	DbUpdateInterval *uint64
	DbWaitTimeout    *uint64
	DbStatInterval   *uint64
	Debug            *bool
}

type ServerConfig struct {
	General General
	Rooms   map[string]RoomConfig
}

func fromConfigFile(configFile string, serverConfig *ServerConfig) {
	_, err := toml.DecodeFile(configFile, &serverConfig)
	if err != nil {
		log.Panicf("Failed to load config file at %s. Make sure the correct file format (toml) is used and the file exists.\nError:%s", configFile, err)
	}
	log.Println("Configurations successfully set.")
}

// what is love
func setDefaults(defaultConfig *General, serverConfig *ServerConfig) {
	if serverConfig.General.Cert == nil {
		serverConfig.General.Cert = defaultConfig.Cert
	}
	if serverConfig.General.DBPath == nil {
		serverConfig.General.DBPath = defaultConfig.DBPath
	}
	if serverConfig.General.DbStatInterval == nil {
		serverConfig.General.DbStatInterval = defaultConfig.DbStatInterval
	}
	if serverConfig.General.DbUpdateInterval == nil {
		serverConfig.General.DbUpdateInterval = defaultConfig.DbUpdateInterval
	}
	if serverConfig.General.DbWaitTimeout == nil {
		serverConfig.General.DbWaitTimeout = defaultConfig.DbWaitTimeout
	}
	if serverConfig.General.Debug == nil {
		serverConfig.General.Debug = defaultConfig.Debug
	}
	if serverConfig.General.Host == nil {
		serverConfig.General.Host = defaultConfig.Host
	}
	if serverConfig.General.Key == nil {
		serverConfig.General.Key = defaultConfig.Key
	}
	if serverConfig.General.Password == nil {
		serverConfig.General.Password = defaultConfig.Password
	}
	if serverConfig.General.Port == nil {
		serverConfig.General.Port = defaultConfig.Port
	}
}

func fromCommandArguments(serverConfig *ServerConfig) {
	if f := flag.CommandLine.Lookup("cert"); f != nil {
		val := f.Value.(flag.Getter).Get().(string)
		serverConfig.General.Cert = &val
	}
	if f := flag.CommandLine.Lookup("dbpath"); f != nil {
		val := f.Value.(flag.Getter).Get().(string)
		serverConfig.General.DBPath = &val
	}
	if f := flag.CommandLine.Lookup("dbstatinterval"); f != nil {
		val := f.Value.(flag.Getter).Get().(uint64)
		serverConfig.General.DbStatInterval = &val
	}
	if f := flag.CommandLine.Lookup("dbupdateinterval"); f != nil {
		val := f.Value.(flag.Getter).Get().(uint64)
		serverConfig.General.DbUpdateInterval = &val
	}
	if f := flag.CommandLine.Lookup("dbwaittimeout"); f != nil {
		val := f.Value.(flag.Getter).Get().(uint64)
		serverConfig.General.DbWaitTimeout = &val
	}
	if f := flag.CommandLine.Lookup("debug"); f != nil {
		val := f.Value.(flag.Getter).Get().(bool)
		serverConfig.General.Debug = &val
	}
	if f := flag.CommandLine.Lookup("host"); f != nil {
		val := f.Value.(flag.Getter).Get().(string)
		serverConfig.General.Host = &val
	}
	if f := flag.CommandLine.Lookup("key"); f != nil {
		val := f.Value.(flag.Getter).Get().(string)
		serverConfig.General.Key = &val
	}
	if f := flag.CommandLine.Lookup("password"); f != nil {
		val := f.Value.(flag.Getter).Get().(string)
		serverConfig.General.Password = &val
	}
	if f := flag.CommandLine.Lookup("port"); f != nil {
		val := f.Value.(flag.Getter).Get().(uint)
		uval := uint16(val)
		serverConfig.General.Port = &uval
	}
}

func printConfig(serverConfig ServerConfig) {
	s, _ := json.MarshalIndent(serverConfig, "", "\t")
	log.Printf("Configuration:\n%s", string(s))
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
	serverConfig.Rooms = make(map[string]RoomConfig, 0)
	if *configFile != "" {
		fromConfigFile(*configFile, &serverConfig)
	}

	var port16 *uint16
	if port == nil {
		port16 = nil
	} else {
		tmp := uint16(*port)
		port16 = &tmp
	}
	defaultConfig := General{Host: host, Port: port16, Cert: cert, Key: key, Password: password, DBPath: dbPath, DbUpdateInterval: dbUpdateInterval, DbWaitTimeout: dbWaitTimeout, DbStatInterval: dbStatInterval, Debug: debug}
	setDefaults(&defaultConfig, &serverConfig)
	fromCommandArguments(&serverConfig)

	printConfig(serverConfig)
	return serverConfig
}
