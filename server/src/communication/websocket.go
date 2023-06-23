package communication

import (
	"context"
	"crypto/tls"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/sevenautumns/niketsu/server/src/logger"
	"nhooyr.io/websocket"
)

// TODO split listener into separate struct
// TODO split http server into separate struct
const unknownUser string = "unknown"

type NewReaderWriter func(*websocket.Conn) WebsocketReaderWriter

type NewClientWorker func(ServerStateHandler, WebsocketReaderWriter, string) ClientWorker

type WebsocketHandler struct {
	handler      ServerStateHandler
	server       *http.Server
	host         string
	port         uint16
	cert         string
	key          string
	readerWriter NewReaderWriter
	clientWorker NewClientWorker
	stopChannel  chan int
	stopSignal   chan os.Signal
	errChannel   chan error
}

func NewWebSocketHandler(config config.GeneralConfig, handler ServerStateHandler, readerWriter NewReaderWriter, clientWorker NewClientWorker) WebsocketHandler {
	var websocketHandler WebsocketHandler
	websocketHandler.host = config.Host
	websocketHandler.port = config.Port
	websocketHandler.cert = config.Cert
	websocketHandler.key = config.Key
	websocketHandler.handler = handler
	websocketHandler.readerWriter = readerWriter
	websocketHandler.clientWorker = clientWorker
	websocketHandler.stopChannel = make(chan int, 1)
	websocketHandler.stopSignal = make(chan os.Signal, 1)
	signal.Notify(websocketHandler.stopSignal, os.Interrupt)
	websocketHandler.errChannel = make(chan error, 1)
	websocketHandler.server = &http.Server{
		Handler:      &websocketHandler,
		ReadTimeout:  time.Second * 10,
		WriteTimeout: time.Second * 10,
	}
	return websocketHandler
}

func (websocketHandler WebsocketHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, nil)
	if err != nil {
		logger.Warnw("Failed to establish connection to client socket", "error", err)
		return
	}

	logger.Infow("New connection established. Creating new worker ...")
	readerWriter := websocketHandler.readerWriter(conn)
	worker := websocketHandler.clientWorker(websocketHandler.handler, readerWriter, unknownUser)
	go worker.Start()
}

func (websocketHandler WebsocketHandler) Stop() {
	close(websocketHandler.stopChannel)
}

func (websocketHandler WebsocketHandler) SigKill() {
	websocketHandler.stopSignal <- syscall.SIGINT
}

func (websocketHandler WebsocketHandler) Close() {
	websocketHandler.server.Close()
}

func (websocketHandler WebsocketHandler) Listen() error {
	useTLS := websocketHandler.cert != "" && websocketHandler.key != ""
	return websocketHandler.listenAndServe(useTLS)
}

func (websocketHandler WebsocketHandler) listenAndServe(useTLS bool) error {
	listener, err := websocketHandler.getListener(useTLS)
	if err != nil {
		return err
	}

	return websocketHandler.serve(listener)
}

func (websocketHandler WebsocketHandler) getListener(useTLS bool) (net.Listener, error) {
	hostPort := fmt.Sprintf("%s:%d", websocketHandler.host, websocketHandler.port)

	var listener net.Listener
	var err error
	if useTLS {
		cert, err := websocketHandler.getCertificate()
		if err != nil {
			logger.Errorw("Failed to load certificate", "error", err)
			return nil, err
		}

		config := &tls.Config{Certificates: []tls.Certificate{cert}}
		listener, err = tls.Listen("tcp", hostPort, config)
	} else {
		listener, err = net.Listen("tcp", hostPort)
	}

	if err != nil {
		logger.Errorw("Failed to create listener", "error", err)
		return nil, err
	}
	logger.Infow("Listening on port", "port", hostPort)
	return listener, nil
}

func (websocketHandler WebsocketHandler) serve(listener net.Listener) error {
	go func() {
		websocketHandler.errChannel <- websocketHandler.server.Serve(listener)
	}()

	select {
	case err := <-websocketHandler.errChannel:
		logger.Warnw("Failed to serve", "error", err)
	case sig := <-websocketHandler.stopSignal:
		logger.Infow("Terminating server", "signal", sig)
	case <-websocketHandler.stopChannel:
		logger.Infow("Terminating server")
	}

	ctx, cancel := context.WithTimeout(context.Background(), time.Second*10)
	defer cancel()

	return websocketHandler.server.Shutdown(ctx)
}

func (websockerHandler WebsocketHandler) getCertificate() (tls.Certificate, error) {
	cert, err := tls.LoadX509KeyPair(websockerHandler.cert, websockerHandler.key)
	return cert, err
}

type WebsocketReaderWriter interface {
	WriteMessage(payload []byte) error
	ReadMessage() ([]byte, error)
	Close() error
}

type WsReaderWriter struct {
	conn *websocket.Conn
}

func NewWsReaderWriter(conn *websocket.Conn) WebsocketReaderWriter {
	return WsReaderWriter{conn: conn}
}

func (webSocket WsReaderWriter) ReadMessage() ([]byte, error) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second*10)
	defer cancel()

	_, payload, err := webSocket.conn.Read(ctx)
	return payload, err
}

func (webSocket WsReaderWriter) WriteMessage(payload []byte) error {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second*10)
	defer cancel()

	err := webSocket.conn.Write(ctx, websocket.MessageText, payload)
	return err
}

func (webSocket WsReaderWriter) Close() error {
	return webSocket.conn.Close(websocket.StatusInternalError, "")
}
