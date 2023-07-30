package communication

import (
	"context"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/golang/mock/gomock"
	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/sevenautumns/niketsu/server/src/db"
	"github.com/stretchr/testify/require"
)

const (
	testDBPath = "pathtodb/"
)

var (
	testConfig = config.CLI{
		Host:             "localhost",
		Port:             1111,
		Cert:             "",
		Key:              "",
		Password:         "password",
		DBPath:           "somepath/db",
		DBUpdateInterval: 1,
		DBWaitTimeout:    1,
		Debug:            false,
	}
	testServerConfig = &serverConfig{
		password:         "password",
		dbPath:           testDBPath,
		dbUpdateInterval: 1,
		dbWaitTimeout:    1,
	}
)

func TestNewServer(t *testing.T) {
	server := NewServer(testConfig.Password, testConfig.DBPath, testConfig.DBUpdateInterval, testConfig.DBWaitTimeout)
	require.NotNil(t, server)
}

func TestServerInit(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockDBManager := db.NewMockDBManager(ctrl)
	server := &Server{
		config:  testServerConfig,
		roomsDB: mockDBManager,
	}
	err := server.Init()
	require.NoError(t, err)
	require.Empty(t, server.rooms)

	t.Cleanup(func() {
		os.RemoveAll(testDBPath)
	})
}

func TestServerShutdown(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)
	mockRoom.EXPECT().
		Shutdown(gomock.Any())

	server := &Server{
		rooms:      map[string]RoomStateHandler{"testRoom": mockRoom},
		roomsMutex: &sync.RWMutex{},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 1*time.Second)
	defer cancel()
	server.Shutdown(ctx)
}

func TestTimeoutServerShutdown(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)
	mockRoom.EXPECT().
		Shutdown(gomock.Any()).
		Do(func(cxt context.Context) {
			time.Sleep(50 * time.Millisecond)
		})

	server := &Server{
		rooms: map[string]RoomStateHandler{
			"testRoom":  mockRoom,
			"testRoom2": mockRoom,
		},
		roomsMutex: &sync.RWMutex{},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	server.Shutdown(ctx)
}

func TestServerAppendRoom(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)
	mockRoom.EXPECT().
		Name().
		Return("testRoom").
		MinTimes(1)

	mockRoom.EXPECT().
		RoomConfig().
		Return(RoomConfig{
			Name:             "testRoom",
			Path:             testDBPath,
			DBUpdateInterval: 0,
			DBWaitTimeout:    0,
			Persistent:       true,
		})

	mockDBManager := db.NewMockDBManager(ctrl)
	mockDBManager.EXPECT().
		Update(gomock.Any(), gomock.Eq("testRoom"), gomock.Any())

	server := &Server{
		rooms:      make(map[string]RoomStateHandler),
		roomsMutex: &sync.RWMutex{},
		roomsDB:    mockDBManager,
	}
	err := server.AppendRoom(mockRoom)
	require.Nil(t, err)
	require.Len(t, server.rooms, 1)
}

func TestServerDeleteRoom(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)
	mockRoom.EXPECT().
		Name().
		Return("testroom").
		MinTimes(1)

	mockDBManager := db.NewMockDBManager(ctrl)
	mockDBManager.EXPECT().
		DeleteKey(gomock.Any(), gomock.Any()).
		Return(nil)

	server := &Server{
		rooms:      map[string]RoomStateHandler{"testroom": mockRoom},
		roomsMutex: &sync.RWMutex{},
		roomsDB:    mockDBManager,
	}
	err := server.DeleteRoom(mockRoom)
	require.Nil(t, err)
	require.Len(t, server.rooms, 0)
}

func TestServerCreateOrFindRoom(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)
	server := &Server{
		rooms:      map[string]RoomStateHandler{"testroom": mockRoom},
		roomsMutex: &sync.RWMutex{},
	}
	room, err := server.CreateOrFindRoom("testroom")
	require.Nil(t, err)
	require.NotNil(t, room)
}

func TestServer_BroadcastStatusList(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)
	mockRoom.EXPECT().
		Name().
		Return("testroom").
		MinTimes(1)

	mockRoom.EXPECT().
		WorkerStatus().
		Return([]Status{})

	mockRoom.EXPECT().
		BroadcastAll(gomock.Any())

	server := &Server{
		rooms:      map[string]RoomStateHandler{"testroom": mockRoom},
		roomsMutex: &sync.RWMutex{},
	}
	server.BroadcastStatusList()
}

func TestServer_IsPasswordCorrect(t *testing.T) {
	server := &Server{
		config: &serverConfig{
			password: "password",
		},
	}
	require.True(t, server.IsPasswordCorrect("password"))
	require.False(t, server.IsPasswordCorrect("wrongpassword"))
}
