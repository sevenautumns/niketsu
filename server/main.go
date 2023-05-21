package main

import (
	server "github.com/sevenautumns/niketsu/server/src"
)

var config server.Config

func init() {
	config = server.GetConfig()
	server.InitLogger(config.General.Debug)
}

func main() {
	defer server.LoggerSync()

	overseer := server.NewOverseer(config)
	overseer.Start()
}
