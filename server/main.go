package main

import (
	"github.com/sevenautumns/niketsu/server/src/communication"
	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

var cli config.CLI

func init() {
	cli = config.ParseCommandArgs()
	config.PrintConfig(cli)
	logger.NewGlobalLogger(cli.Debug)
}

func main() {
	defer logger.Sync()

	server := communication.NewServer(cli.Password, cli.DBPath, cli.DBUpdateInterval, cli.DBWaitTimeout)
	err := server.Init()
	if err != nil {
		logger.Fatalw("Failed to initialize handler", "error", err)
	}
	handler := communication.NewWebSocketHandler(cli.Host, cli.Port, cli.Cert, cli.Key,
		server, communication.NewWsReaderWriter, communication.NewWorker)
	err = handler.Listen()
	if err != nil {
		logger.Fatalw("Shutting down server", "error", err)
	}
}
