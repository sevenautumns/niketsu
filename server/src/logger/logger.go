package logger

import (
	"log"

	"go.uber.org/zap"
	"go.uber.org/zap/zaptest/observer"
)

type Logger interface {
	Infow(msg string, keysAndValues ...interface{})
	Debugw(msg string, keysAndValues ...interface{})
	Warnw(msg string, keysAndValues ...interface{})
	Errorw(msg string, keysAndValues ...interface{})
	Panicw(msg string, keysAndValues ...interface{})
	Fatalw(msg string, keysAndValues ...interface{})
	Sync() error
}

var logger Logger

func init() {
	core, _ := observer.New(zap.InfoLevel)
	logger = zap.New(core).Sugar()
}

// Initialize the logger once, so it can be used in other packages.
func NewGlobalLogger(debug bool) {
	var err error
	var zapLogger *zap.Logger

	if debug {
		zapLogger, err = zap.NewDevelopment()
	} else {
		zapLogger, err = zap.NewProduction()
	}

	if err != nil {
		log.Fatalf("Failed to initialize zap logger: %v", err)
	}
	logger = zapLogger.Sugar()
}

func Infow(msg string, keysAndValues ...interface{}) {
	logger.Infow(msg, keysAndValues...)
}

func Debugw(msg string, keysAndValues ...interface{}) {
	logger.Debugw(msg, keysAndValues...)
}

func Warnw(msg string, keysAndValues ...interface{}) {
	logger.Warnw(msg, keysAndValues...)
}

func Errorw(msg string, keysAndValues ...interface{}) {
	logger.Errorw(msg, keysAndValues...)
}

func Panicw(msg string, keysAndValues ...interface{}) {
	logger.Panicw(msg, keysAndValues...)
}

func Fatalw(msg string, keysAndValues ...interface{}) {
	logger.Fatalw(msg, keysAndValues...)
}

func Sync() {
	logger.Sync()
}
