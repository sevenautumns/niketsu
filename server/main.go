package main

import (
	server "github.com/sevenautumns/niketsu/server/src"
)

func main() {
	serverConfig, playlistConfig := server.GetConfig()
	server.InitLogger(serverConfig.Debug)
	defer server.LoggerSync()

	capitalist := server.NewCapitalist(serverConfig.Host, serverConfig.Port, serverConfig.Password, playlistConfig)
	capitalist.Start()
}
