package main

import (
	"github.com/sevenautumns/niketsu/server/src/communication"
	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

var conf config.Config

func init() {
	conf = config.GetConfig()
	logger.NewGlobalLogger(conf.General.Debug)
}

func main() {
	defer logger.Sync()

	server := communication.NewServer(conf.General)
	err := server.Init(conf.Rooms)
	if err != nil {
		logger.Fatalw("Failed to initialize handler", "error", err)
	}
	handler := communication.NewWebSocketHandler(conf.General, server, communication.NewWsReaderWriter, communication.NewWorker)
	err = handler.Listen()
	if err != nil {
		logger.Fatalw("Shutting down server", "error", err)
	}
}
