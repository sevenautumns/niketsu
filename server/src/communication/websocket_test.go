package communication

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/stretchr/testify/require"
	"nhooyr.io/websocket"
	"nhooyr.io/websocket/wsjson"
)

const (
	host       string = "localhost"
	portTCP    uint16 = 7766
	portTLS    uint16 = 7777
	cert       string = "testdata/certificate.crt"
	key        string = "testdata/private.key"
	testCases  int    = 10
	failedCert string = "thisdoesnotexist"
	failedKey  string = "thisalsodoesnotexist"
	failedHost string = "incorrect port"
)

var (
	testConfigTCP = config.GeneralConfig{
		Host: host,
		Port: portTCP,
	}
	testConfigTLS = config.GeneralConfig{
		Host: host,
		Port: portTLS,
		Cert: cert,
		Key:  key,
	}
	testFailedCertificateConfig = config.GeneralConfig{
		Host: host,
		Port: portTLS,
		Cert: failedCert,
		Key:  failedKey,
	}
	testFailedHostPortConfig = config.GeneralConfig{
		Host: failedHost,
		Port: portTCP,
	}
)

type MockServerStateHandler struct{}

func (ms MockServerStateHandler) DeleteRoom(room RoomStateHandler) {}

func (ms MockServerStateHandler) HandleJoin(join Join, worker ClientWorker) {}

func (ms MockServerStateHandler) BroadcastStatusList(worker ClientWorker) {}

type MockClientWorker struct {
	websocket WebsocketReaderWriter
}

func (mcw MockClientWorker) GetUUID() *uuid.UUID { return &uuid.UUID{} }

func (mcw MockClientWorker) SetUserStatus(status Status) {}

func (mcw MockClientWorker) GetUserStatus() *Status { return &Status{} }

func (mcw MockClientWorker) GetVideoState() *videoState { return &videoState{} }

func (mcw MockClientWorker) SetVideoState(videoStatus VideoStatus, arrivalTime time.Time) {}

func (mcw MockClientWorker) Login() {}

func (mcw MockClientWorker) IsLoggedIn() bool { return false }

func (mcw MockClientWorker) SendMessage(payload []byte) {}

func (mcw MockClientWorker) SendServerMessage(message string, isError bool) {}

func (mcw MockClientWorker) SendSeek(desync bool) {}

func (mcw MockClientWorker) SendPlaylist() {}

func (mcw MockClientWorker) EstimatePosition() uint64 { return 0 }

func (mcw MockClientWorker) DeleteWorkerFromRoom() {}

func (mcw MockClientWorker) SetRoom(room RoomStateHandler) {}

func (mcw MockClientWorker) Close() {}

func (mcw MockClientWorker) Start() {
	defer mcw.websocket.Close()
	for i := 0; i < testCases; i++ {
		msg, err := mcw.websocket.ReadMessage()
		if err != nil {
			log.Fatalf("Failed to read message: %s", err)
		}
		err = mcw.websocket.WriteMessage(msg)
		if err != nil {
			log.Fatalf("Failed to read message: %s", err)
		}
	}

}

func NewMockWorker(roomHandler ServerStateHandler, webSocket WebsocketReaderWriter, userName string) ClientWorker {
	var worker MockClientWorker
	worker.websocket = webSocket
	return worker
}

func TestFailedCertificate(t *testing.T) {
	handler := NewWebSocketHandler(testFailedCertificateConfig, MockServerStateHandler{}, NewWsReaderWriter, NewMockWorker)
	err := handler.Listen()
	require.Error(t, err)
}

func TestFailedHostPort(t *testing.T) {
	t.Parallel()
	handler := NewWebSocketHandler(testFailedHostPortConfig, MockServerStateHandler{}, NewWsReaderWriter, NewMockWorker)
	err := handler.Listen()
	require.Error(t, err)
}

func TestStop(t *testing.T) {
	handler := NewWebSocketHandler(testConfigTCP, MockServerStateHandler{}, NewWsReaderWriter, NewMockWorker)
	stopChannel := make(chan int, 1)
	go listenChannel(t, handler, stopChannel)
	handler.Stop()
	<-stopChannel
}

func TestSigKill(t *testing.T) {
	handler := NewWebSocketHandler(testConfigTCP, MockServerStateHandler{}, NewWsReaderWriter, NewMockWorker)
	stopChannel := make(chan int, 1)
	go listenChannel(t, handler, stopChannel)
	handler.SigKill()
	<-stopChannel
}

func TestClose(t *testing.T) {
	handler := NewWebSocketHandler(testConfigTCP, MockServerStateHandler{}, NewWsReaderWriter, NewMockWorker)
	stopChannel := make(chan int, 1)
	go listenChannel(t, handler, stopChannel)
	handler.Close()
	<-stopChannel
}

func TestListenTLS(t *testing.T) {
	handler := NewWebSocketHandler(testConfigTLS, MockServerStateHandler{}, NewWsReaderWriter, NewMockWorker)
	url := fmt.Sprintf("wss://%s:%d", host, portTLS)
	testListen(t, handler, url)
}

func TestListenTCP(t *testing.T) {
	handler := NewWebSocketHandler(testConfigTCP, MockServerStateHandler{}, NewWsReaderWriter, NewMockWorker)
	url := fmt.Sprintf("ws://%s:%d", host, portTCP)
	testListen(t, handler, url)
}

func testListen(t *testing.T, handler WebsocketHandler, url string) {
	go listen(t, handler)
	defer handler.Stop()

	ctx, cancel := context.WithTimeout(context.Background(), time.Second*10)
	defer cancel()

	time.Sleep(2 * time.Second) // Wait for handler initialization
	conn, _, err := websocket.Dial(ctx, url, nil)
	require.NoError(t, err)

	testReadWrite(ctx, t, conn)
}

func listen(t *testing.T, handler WebsocketHandler) {
	err := handler.Listen()
	require.NoError(t, err)
}

func testReadWrite(ctx context.Context, t *testing.T, conn *websocket.Conn) {
	defer conn.Close(websocket.StatusInternalError, "failure ...")

	for i := 0; i < testCases; i++ {
		err := wsjson.Write(ctx, conn, map[string]int{
			"i": i,
		})
		require.NoError(t, err)

		v := map[string]int{}
		err = wsjson.Read(ctx, conn, &v)
		require.NoError(t, err)
		require.Equal(t, i, v["i"])
	}

	conn.Close(websocket.StatusNormalClosure, "")
}

func listenChannel(t *testing.T, handler WebsocketHandler, stopChannel chan int) {
	defer close(stopChannel)
	err := handler.Listen()
	require.NoError(t, err)
}
