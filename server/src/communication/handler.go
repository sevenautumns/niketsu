package communication

import (
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sync"
	"time"

	"github.com/sevenautumns/niketsu/server/src/config"
	"github.com/sevenautumns/niketsu/server/src/db"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

const (
	generalDBPath   string = ".main/general.db"
	generalDBBucket string = "general"
)

type ServerStateHandler interface {
	DeleteRoom(room RoomStateHandler)
	HandleJoin(join Join, worker ClientWorker)
	BroadcastStatusList(worker ClientWorker)
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

// Creates server based on config.
// May fail to initialize and return error if directory or database creation fail.
// Call Init() before doing anything else to initialize the database, etc.
func NewServer(config config.GeneralConfig) Server {
	var server Server
	server.config = &serverConfig{password: config.Password, dbPath: config.DBPath, dbUpdateInterval: config.DBUpdateInterval, dbWaitTimeout: config.DBWaitTimeout}

	return server
}

func (server *Server) Init(roomConfigs map[string]config.RoomConfig) error {
	err := CreateDir(server.config.dbPath)
	if err != nil {
		return errors.New(fmt.Sprintf("Failed to create directory of db path %s\n%s", server.config.dbPath, err))
	}

	path := filepath.Join(server.config.dbPath, generalDBPath)
	err = CreateDir(path)
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
	kevValueStore, err := db.NewBoltKeyValueStore(path, server.config.dbWaitTimeout)
	if err != nil {
		return err
	}
	server.roomsDB = db.NewDBManager(kevValueStore)

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
		newRoom.Start()
	}

	return rooms
}

func (server *Server) createOrFindRoom(roomName string) (RoomStateHandler, error) {
	if server.rooms[roomName] == nil {
		tmpRoom, err := NewRoom(roomName, server.config.dbPath, server.config.dbUpdateInterval, server.config.dbWaitTimeout, false) //new rooms are never persistent
		if err != nil {
			return nil, err
		}

		return tmpRoom, nil
	} else {
		return server.rooms[roomName], nil
	}
}

func (server Server) HandleJoin(join Join, worker ClientWorker) {
	logger.Debugw("Received login attempt", "message", join)
	if server.passwordCheckFailed(join.Password) {
		worker.SendServerMessage("Password is incorrect. Please try again", true)
		return
	}

	var err error
	if worker.LoggedIn() {
		err = server.handleRoomChange(join, worker)
	} else {
		err = server.handleFirstLogin(join, worker)
	}

	if err != nil {
		logger.Infow("Room change failed", "error", err)
		worker.SendServerMessage("Failed to access room. Please try again", true)
	}
}

func (server *Server) passwordCheckFailed(password string) bool {
	return server.config.password != "" && password != server.config.password
}

func (server *Server) handleRoomChange(join Join, worker ClientWorker) error {
	worker.DeleteWorkerFromRoom()
	err := server.updateRoomChangeState(join.Room, worker)
	if err != nil {
		return err
	}

	server.sendRoomChangeUpdates(worker)
	return nil
}

func (server *Server) handleFirstLogin(join Join, worker ClientWorker) error {
	// it is important to first set the state and then login.
	// Otherwise, messages from the client may be handle with an incorrect state
	err := server.updateRoomChangeState(join.Room, worker)
	if err != nil {
		return err
	}
	worker.SetUserStatus(Status{Ready: false, Username: join.Username}) // update username based on join
	worker.Login()
	server.sendRoomChangeUpdates(worker)
	return nil
}

func (server *Server) sendRoomChangeUpdates(worker ClientWorker) {
	server.BroadcastStatusList(worker)
	worker.SendPlaylist()
	worker.SendSeek(true)
}

func (server *Server) updateRoomChangeState(roomName string, worker ClientWorker) error {
	room, err := server.createOrFindRoom(roomName)
	if err != nil {
		return err
	}

	room.Start()
	server.appendRoom(room)
	server.writeRoom(room)
	room.AppendWorker(worker)

	roomState := room.RoomState()
	worker.SetVideoState(VideoStatus{Filename: roomState.video, Position: roomState.position, Paused: roomState.paused}, time.Now())
	worker.SetRoom(room)

	return nil
}

func (server *Server) appendRoom(room RoomStateHandler) {
	server.roomsMutex.Lock()
	defer server.roomsMutex.Unlock()

	server.rooms[room.Name()] = room
}

func (server *Server) writeRoom(room RoomStateHandler) {
	server.roomsMutex.RLock()
	defer server.roomsMutex.RUnlock()

	//needs to be extended in case more options are added to room, e.g. a room config
	config := config.RoomConfig{Persistent: room.IsPersistent()}
	byteConfig, err := json.Marshal(config)
	if err != nil {
		logger.Warnw("Failed to marshal room config", "error", err)
		return
	}

	err = server.roomsDB.Update(generalDBBucket, room.Name(), byteConfig)
	if err != nil {
		logger.Warnw("Update key/value transaction for room configurations failed", "error", err)
	}
}

func (server Server) DeleteRoom(room RoomStateHandler) {
	server.roomsMutex.Lock()
	defer server.roomsMutex.Unlock()

	roomName := room.Name()
	delete(server.rooms, roomName)
	err := server.deleteRoomFromDB(room)
	if err != nil {
		logger.Warnw("Failed to delete the room configuration from the general database", "room", roomName, "error", err)
	}

	err = room.Close()
	if err != nil {
		logger.Warnw("Failed to delete the database of room", "room", roomName, "error", err)
	}
}

func (server *Server) deleteRoomFromDB(room RoomStateHandler) error {
	return server.roomsDB.DeleteKey(generalDBBucket, room.Name())
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

// StatusList is also sent to the client who sent the last VideoStatus
func (server Server) BroadcastStatusList(worker ClientWorker) {
	rooms := server.statusList()

	userStatus := worker.UserStatus()
	statusList := StatusList{Rooms: rooms, Username: userStatus.Username}
	message, err := MarshalMessage(statusList)
	if err != nil {
		logger.Errorw("Unable to parse status list", "error", err)
		return
	}

	server.broadcastAll(message)
}

func (server *Server) broadcastAll(message []byte) {
	server.roomsMutex.RLock()
	defer server.roomsMutex.RUnlock()

	for _, room := range server.rooms {
		room.BroadcastAll(message)
	}
}
