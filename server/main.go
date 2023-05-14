package main

import (
	server "github.com/sevenautumns/niketsu/server/src"
)

var config server.ServerConfig

func init() {
	config = server.GetConfig()
	server.InitLogger(config.General.Debug)
}

func main() {
	defer server.LoggerSync()

	capitalist := server.NewCapitalist(config)
	capitalist.Start()
}
