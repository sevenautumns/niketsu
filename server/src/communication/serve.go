package communication

import (
	"errors"
	"fmt"
	"net/http"

	"github.com/gobwas/ws"
	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

type RequestHandler interface {
	HandleRequests()
}

type WebsocketHandler struct {
	server ServerStateHandler
	host   string
	port   uint16
	cert   string
	key    string
}

func NewWebSocketHandler(config config.GeneralConfig, server ServerStateHandler) WebsocketHandler {
	var webSocketHandler WebsocketHandler
	webSocketHandler.host = config.Host
	webSocketHandler.port = config.Port
	webSocketHandler.cert = config.Cert
	webSocketHandler.key = config.Key
	webSocketHandler.server = server

	return webSocketHandler
}

func (webSocketHandler WebsocketHandler) HandleRequests() {
	if webSocketHandler.cert == "" || webSocketHandler.key == "" {
		webSocketHandler.listenAndServe()
	} else {
		webSocketHandler.listenAndServeTLS()
	}
}

func (webSocketHandler WebsocketHandler) listenAndServe() {
	hostPort := fmt.Sprintf("%s:%d", webSocketHandler.host, webSocketHandler.port)

	logger.Infow("Finished initializing manager. Starting http listener ...")
	err := http.ListenAndServe(hostPort, http.HandlerFunc(webSocketHandler.handler))

	if errors.Is(err, http.ErrServerClosed) {
		logger.Infow("Server closed connection")
	} else if err != nil {
		logger.Fatalw("Error starting server", "error", err)
	}
}

func (webSocketHandler WebsocketHandler) listenAndServeTLS() {
	hostPort := fmt.Sprintf("%s:%d", webSocketHandler.host, webSocketHandler.port)

	logger.Infow("Finished initializing manager. Starting tls listener ...")
	err := http.ListenAndServeTLS(hostPort, webSocketHandler.cert, webSocketHandler.key, http.HandlerFunc(webSocketHandler.handler))

	if errors.Is(err, http.ErrServerClosed) {
		logger.Infow("Server closed connection")
	} else if err != nil {
		logger.Fatalw("Error starting server", "error", err)
	}
}

func (webSocketHandler WebsocketHandler) handler(w http.ResponseWriter, r *http.Request) {
	conn, _, _, err := ws.UpgradeHTTP(r, w)
	if err != nil {
		logger.Errorw("Failed to establish connection to client socket", "error", err)
		return
	}

	logger.Infow("New connection established. Creating new worker ...")
	webSocket := WsWebSocket{conn: conn}
	worker := NewWorker(webSocketHandler.server, webSocket, "unknown", nil, nil)
	go worker.Start()
}
