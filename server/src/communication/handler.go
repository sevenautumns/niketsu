package communication

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sync"

	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/sevenautumns/niketsu/server/src/db"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

const (
	generalDBPath   string = ".db/general.db"
	generalDBBucket string = "general"
)

type ServerStateHandler interface {
	Init(roomConfigs map[string]config.RoomConfig) error
	Shutdown(ctx context.Context)
	DeleteRoom(room RoomStateHandler) error
	AppendRoom(room RoomStateHandler) error
	CreateOrFindRoom(roomName string) (RoomStateHandler, error)
	BroadcastStatusList()
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

func NewServer(config config.GeneralConfig) ServerStateHandler {
	var server Server
	server.config = &serverConfig{password: config.Password, dbPath: config.DBPath, dbUpdateInterval: config.DBUpdateInterval, dbWaitTimeout: config.DBWaitTimeout}

	return &server
}

func (server *Server) Init(roomConfigs map[string]config.RoomConfig) error {
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
	server.addRooms(roomConfigs)

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

func (server *Server) addRooms(roomConfigs map[string]config.RoomConfig) {
	rooms := make(map[string]RoomStateHandler, 0)
	rooms = server.addRoomsFromDB(rooms)
	rooms = server.addRoomsFromConfig(rooms, roomConfigs)

	server.rooms = rooms
}

func (server *Server) addRoomsFromDB(rooms map[string]RoomStateHandler) map[string]RoomStateHandler {
	roomConfigs, err := server.roomsDB.GetRoomConfigs(generalDBBucket)
	if err != nil {
		logger.Warnw("Failed to retrieve room configurations from database", "error", err)
		return rooms
	}

	for name, roomConfig := range roomConfigs {
		newRoom, err := NewRoom(name, server.config.dbPath, server.config.dbUpdateInterval, server.config.dbWaitTimeout, roomConfig.Persistent)
		if err != nil {
			continue
		}

		rooms[name] = newRoom
		go newRoom.Start()
	}

	return rooms
}

func (server *Server) addRoomsFromConfig(rooms map[string]RoomStateHandler, roomConfigs map[string]config.RoomConfig) map[string]RoomStateHandler {
	for name, roomConfig := range roomConfigs {
		if _, ok := rooms[name]; ok {
			continue
		}

		newRoom, err := NewRoom(name, server.config.dbPath, server.config.dbUpdateInterval, server.config.dbWaitTimeout, roomConfig.Persistent)
		if err != nil {
			continue
		}

		server.writeRoom(newRoom)
		rooms[name] = newRoom
		go newRoom.Start()
	}

	return rooms
}

func (server *Server) writeRoom(room RoomStateHandler) error {
	//needs to be extended in case more options are added to room, e.g. a room config
	config := config.RoomConfig{Persistent: room.IsPersistent()}
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
