package communication

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sync"

	"github.com/brianvoe/gofakeit/v6"
	"github.com/sevenautumns/niketsu/server/src/db"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

var usernamePrefix = []string{"BitWarden", "Jolly", "Funky", "Spacy", "Mean", "Machine", "ByteCode", "PixelPerfect", "CryptoBro", "CryptoJunky", "Chroot", "BitBard", "DebugCowboy", "Buggy", "Magician", "BinaryBaron", "Fluid"}

const (
	generalDBPath   string = ".db/general.db"
	generalDBBucket string = "general"
)

type ServerStateHandler interface {
	Init() error
	Shutdown(ctx context.Context)
	DeleteRoom(room RoomStateHandler) error
	AppendRoom(room RoomStateHandler) error
	CreateOrFindRoom(roomName string) (RoomStateHandler, error)
	BroadcastStatusList()
	RenameUserIfUnavailable(username string) string
	IsPasswordCorrect(password string) bool
}

type Server struct {
	config     *serverConfig
	rooms      map[string]RoomStateHandler
	roomsMutex *sync.RWMutex
	roomsDB    db.DBManager
}

type serverConfig struct {
	password         string
	dbPath           string
	dbUpdateInterval uint64
	dbWaitTimeout    uint64
}

func NewServer(password string, dbPath string, dbUpdateInterval uint64, dbWaitTimeout uint64) ServerStateHandler {
	var server Server
	server.config = &serverConfig{password: password, dbPath: dbPath, dbUpdateInterval: dbUpdateInterval, dbWaitTimeout: dbWaitTimeout}

	return &server
}

func (server *Server) Init() error {
	err := CreateDir(server.config.dbPath)
	if err != nil {
		return errors.New(fmt.Sprintf("Failed to create directory of db path %s\n%s", server.config.dbPath, err))
	}

	path := filepath.Join(server.config.dbPath, generalDBPath)
	err = CreateDir(filepath.Dir(path))
	if err != nil {
		return errors.New(fmt.Sprintf("Failed to create directory of general db path %s\n%s", path, err))
	}

	err = server.initNewRoomsDB(path)
	if err != nil {
		return errors.New(fmt.Sprintf("Failed to initialize database for rooms\n%s", err))
	}

	server.roomsMutex = &sync.RWMutex{}
	server.rooms = make(map[string]RoomStateHandler, 0)
	server.addRoomsFromDB()

	return nil
}

func (server *Server) initNewRoomsDB(path string) error {
	db, err := db.NewDBManager(path, server.config.dbWaitTimeout)
	if err != nil {
		return err
	}
	server.roomsDB = db

	err = server.roomsDB.Open()
	if err != nil {
		return err
	}

	return nil
}

func (server *Server) addRoomsFromDB() {
	bucketValues, err := server.roomsDB.GetAll(generalDBBucket)
	if err != nil {
		logger.Warnw("Failed to retrieve room configurations from database", "error", err)
		return
	}

	for name, roomConfigBytes := range bucketValues {
		var roomConfig RoomConfig
		err := json.Unmarshal(roomConfigBytes, &roomConfig)
		if err != nil {
			logger.Warnw("Failed to marshal room configuration from DB", "name", name)
			continue
		}

		logger.Debugw("Retrieved room config", "config", roomConfig)
		newRoom, err := NewRoom(roomConfig.Name, roomConfig.Path, uint64(roomConfig.DBUpdateInterval), roomConfig.DBWaitTimeout, roomConfig.Persistent)
		if err != nil {
			continue
		}

		logger.Debugw("Room initialized with playlist from db", "state", newRoom.RoomState())
		server.rooms[roomConfig.Name] = newRoom
		go newRoom.Start()
	}
}

func (server *Server) writeRoom(room RoomStateHandler) error {
	config := room.RoomConfig()
	byteConfig, err := json.Marshal(config)
	if err != nil {
		return err
	}

	err = server.roomsDB.Update(generalDBBucket, room.Name(), byteConfig)
	if err != nil {
		return err
	}

	return nil
}

func (server *Server) Shutdown(ctx context.Context) {
	server.roomsMutex.Lock()
	defer server.roomsMutex.Unlock()

	for _, room := range server.rooms {
		select {
		case <-ctx.Done():
			return
		default:
			room.Shutdown(ctx)
		}
	}
}

func (server *Server) AppendRoom(room RoomStateHandler) error {
	server.roomsMutex.Lock()
	defer server.roomsMutex.Unlock()

	_, ok := server.rooms[room.Name()]
	if !ok {
		server.rooms[room.Name()] = room
		return server.writeRoom(room)
	}

	return nil
}

func (server *Server) DeleteRoom(room RoomStateHandler) error {
	server.roomsMutex.Lock()
	defer server.roomsMutex.Unlock()

	delete(server.rooms, room.Name())
	err := server.deleteRoomFromDB(room.Name())
	if err != nil {
		return err
	}
	return nil
}

func (server *Server) deleteRoomFromDB(roomName string) error {
	return server.roomsDB.DeleteKey(generalDBBucket, roomName)
}

func (server *Server) CreateOrFindRoom(roomName string) (RoomStateHandler, error) {
	room, ok := server.rooms[roomName]
	if !ok {
		tmpRoom, err := NewRoom(roomName, server.config.dbPath, server.config.dbUpdateInterval, server.config.dbWaitTimeout, false) //new rooms are never persistent
		if err != nil {
			return nil, err
		}
		go tmpRoom.Start()

		return tmpRoom, nil
	} else {
		return room, nil
	}
}

func (server Server) BroadcastStatusList() {
	rooms := server.statusList()

	statusList := StatusList{Rooms: rooms}
	message, err := MarshalMessage(statusList)
	if err != nil {
		logger.Errorw("Unable to parse status list", "error", err)
		return
	}

	server.broadcastAll(message)
}

func (server *Server) statusList() map[string][]Status {
	server.roomsMutex.RLock()
	defer server.roomsMutex.RUnlock()

	statusList := make(map[string][]Status, 0)
	for _, room := range server.rooms {
		statusList[room.Name()] = room.WorkerStatus()
	}

	return statusList
}

func (server *Server) broadcastAll(message []byte) {
	server.roomsMutex.RLock()
	defer server.roomsMutex.RUnlock()

	for _, room := range server.rooms {
		room.BroadcastAll(message)
	}
}

func (server *Server) IsPasswordCorrect(password string) bool {
	return server.config.password == "" || password == server.config.password
}

func (server *Server) RenameUserIfUnavailable(username string) string {
	rooms := server.statusList()
	usernameMap := make(map[string]bool, 0)

	for _, roomStatus := range rooms {
		for _, status := range roomStatus {
			usernameMap[status.Username] = true
		}
	}

	if usernameMap[username] {
		return server.chooseNewUsername(username, usernameMap)
	}

	return username
}

func (server *Server) chooseNewUsername(username string, usernameMap map[string]bool) string {
	for i := 0; i < 100; i++ {
		randomPrefix := gofakeit.BuzzWord()
		newUsername := fmt.Sprintf("%s %s", randomPrefix, username)

		if !usernameMap[newUsername] {
			return newUsername
		}
	}

	return fmt.Sprintf("%s_%s", username, gofakeit.UUID())
}
