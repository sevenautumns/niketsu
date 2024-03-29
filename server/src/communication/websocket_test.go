package communication

import (
	"context"
	"fmt"
	"log"
	"sync"
	"testing"
	"time"

	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/stretchr/testify/require"
	gomock "go.uber.org/mock/gomock"
	"nhooyr.io/websocket"
	"nhooyr.io/websocket/wsjson"
)

const (
	host       string = "localhost"
	portTCP    uint16 = 7760
	portTCP2   uint16 = 7767
	portTCP3   uint16 = 7768
	portTCP4   uint16 = 7769
	portTLS    uint16 = 7777
	cert       string = "testdata/certificate.crt"
	key        string = "testdata/private.key"
	testCases  int    = 10
	failedCert string = "thisdoesnotexist"
	failedKey  string = "thisalsodoesnotexist"
	failedHost string = "incorrect port"
)

var (
	testConfigTCP = config.CLI{
		Host: host,
		Port: portTCP,
	}
	testConfigTCP2 = config.CLI{
		Host: host,
		Port: portTCP2,
	}
	testConfigTCP3 = config.CLI{
		Host: host,
		Port: portTCP3,
	}
	testConfigTLS = config.CLI{
		Host: host,
		Port: portTLS,
		Cert: cert,
		Key:  key,
	}
	testFailedCertificateConfig = config.CLI{
		Host: host,
		Port: portTLS,
		Cert: failedCert,
		Key:  failedKey,
	}
	testFailedHostPortConfig = config.CLI{
		Host: failedHost,
		Port: portTCP4,
	}
)

func newMockClientWorkerWrapper(ctrl *gomock.Controller) func(roomHandler ServerStateHandler, webSocket WebsocketReaderWriter, userName string) ClientWorker {
	mockClientWorker := NewMockClientWorker(ctrl)

	return func(serverStateHandler ServerStateHandler, websocketReaderWriter WebsocketReaderWriter, name string) ClientWorker {
		mockClientWorker.EXPECT().
			Start().
			Do(func() {
				defer websocketReaderWriter.Close()
				for i := 0; i < testCases; i++ {
					msg, err := websocketReaderWriter.ReadMessage()
					if err != nil {
						log.Fatalf("Failed to read message: %s", err)
					}
					err = websocketReaderWriter.WriteMessage(msg)
					if err != nil {
						log.Fatalf("Failed to read message: %s", err)
					}
				}
			})
		return mockClientWorker
	}
}

func TestFailedCertificate(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockServerStateHandler := NewMockServerStateHandler(ctrl)
	newMockClientWorker := newMockClientWorkerWrapper(ctrl)

	handler := NewWebSocketHandler(testFailedCertificateConfig.Host, testFailedCertificateConfig.Port,
		testFailedCertificateConfig.Cert, testFailedCertificateConfig.Key,
		mockServerStateHandler, NewWsReaderWriter, newMockClientWorker)
	err := handler.Listen()
	require.Error(t, err)
}

func TestFailedHostPort(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockServerStateHandler := NewMockServerStateHandler(ctrl)
	newMockClientWorker := newMockClientWorkerWrapper(ctrl)
	handler := NewWebSocketHandler(testFailedHostPortConfig.Host, testFailedHostPortConfig.Port,
		testFailedHostPortConfig.Cert, testFailedHostPortConfig.Key,
		mockServerStateHandler, NewWsReaderWriter, newMockClientWorker)
	err := handler.Listen()
	require.Error(t, err)
}

func TestStop(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockServerStateHandler := NewMockServerStateHandler(ctrl)
	mockServerStateHandler.EXPECT().
		Shutdown(gomock.Any())

	newMockClientWorker := newMockClientWorkerWrapper(ctrl)
	handler := NewWebSocketHandler(testConfigTCP.Host, testConfigTCP.Port, testConfigTCP.Cert, testConfigTCP.Key,
		mockServerStateHandler, NewWsReaderWriter, newMockClientWorker)

	var wg sync.WaitGroup
	wg.Add(1)
	go listenChannel(t, handler, &wg)
	handler.Stop()
	wg.Wait()
}

func TestClose(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockServerStateHandler := NewMockServerStateHandler(ctrl)
	mockServerStateHandler.EXPECT().
		Shutdown(gomock.Any())

	newMockClientWorker := newMockClientWorkerWrapper(ctrl)
	handler := NewWebSocketHandler(testConfigTCP2.Host, testConfigTCP2.Port, testConfigTCP2.Cert, testConfigTCP2.Key,
		mockServerStateHandler, NewWsReaderWriter, newMockClientWorker)

	var wg sync.WaitGroup
	wg.Add(1)
	go listenChannel(t, handler, &wg)
	handler.Stop()
	wg.Wait()
}

func TestListenTLS(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockServerStateHandler := NewMockServerStateHandler(ctrl)
	mockServerStateHandler.EXPECT().
		Shutdown(gomock.Any())

	newMockClientWorker := newMockClientWorkerWrapper(ctrl)
	handler := NewWebSocketHandler(testConfigTLS.Host, testConfigTLS.Port, testConfigTLS.Cert, testConfigTLS.Key,
		mockServerStateHandler, NewWsReaderWriter, newMockClientWorker)
	url := fmt.Sprintf("wss://%s:%d", host, portTLS)
	testListen(t, handler, url)
}

func TestListenTCP(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockServerStateHandler := NewMockServerStateHandler(ctrl)
	mockServerStateHandler.EXPECT().
		Shutdown(gomock.Any())

	newMockClientWorker := newMockClientWorkerWrapper(ctrl)
	handler := NewWebSocketHandler(testConfigTCP3.Host, testConfigTCP3.Port, testConfigTCP3.Cert, testConfigTCP3.Key,
		mockServerStateHandler, NewWsReaderWriter, newMockClientWorker)
	url := fmt.Sprintf("ws://%s:%d", host, portTCP3)
	testListen(t, handler, url)
}

func testListen(t *testing.T, handler WebsocketHandler, url string) {
	var wg sync.WaitGroup
	wg.Add(1)
	go listen(t, handler, &wg)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second*10)
	defer cancel()

	time.Sleep(time.Second) // Wait for handler initialization
	conn, _, err := websocket.Dial(ctx, url, nil)
	require.NoError(t, err)

	testReadWrite(ctx, t, conn)
	handler.Stop()
	wg.Wait()
}

func listen(t *testing.T, handler WebsocketHandler, wg *sync.WaitGroup) {
	err := handler.Listen()
	require.NoError(t, err)
	wg.Done()
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

func listenChannel(t *testing.T, handler WebsocketHandler, wg *sync.WaitGroup) {
	err := handler.Listen()
	require.NoError(t, err)
	wg.Done()
}
