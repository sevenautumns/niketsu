package niketsu_server

import (
	"log"

	"go.uber.org/zap"
)

var logger *zap.SugaredLogger

func InitLogger(debug bool) {
	var err error
	var zapLogger *zap.Logger

	if debug {
		zapLogger, err = zap.NewDevelopment()
	} else {
		zapLogger, err = zap.NewProduction()
	}

	if err != nil {
		log.Fatalf("can't initialize zap logger: %v", err)
	}
	logger = zapLogger.Sugar()
}

func LoggerSync() {
	logger.Sync()
}
