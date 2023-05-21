package niketsu_server

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/gobwas/ws"
)

const (
	GENERAL_DB_PATH string = ".main/general.db"
)

type Overseer struct {
	config     GeneralConfig
	rooms      map[string]*Room
	roomsMutex *sync.RWMutex
	roomsDB    *DB
}

func NewOverseer(serverConfig Config) Overseer {
	var overseer Overseer
	config := serverConfig.General
	overseer.config = config
	overseer.createDir(overseer.config.DBPath)
	generalDBPath := filepath.Join(config.DBPath, GENERAL_DB_PATH)
	overseer.createDir(generalDBPath)
	overseer.createNewRoomsDB(generalDBPath)
	overseer.roomsMutex = &sync.RWMutex{}
	overseer.addRooms(serverConfig.Rooms)

	return overseer
}

func (overseer *Overseer) Start() {
	hostPort := fmt.Sprintf("%s:%d", overseer.config.Host, overseer.config.Port)
	if overseer.config.Cert == "" || overseer.config.Key == "" {
		logger.Info("Finished initializing manager. Starting http listener ...")
		http.ListenAndServe(hostPort, http.HandlerFunc(overseer.handler))
	} else {
		logger.Info("Finished initializing manager. Starting tls listener ...")
		http.ListenAndServeTLS(hostPort, overseer.config.Cert, overseer.config.Key, http.HandlerFunc(overseer.handler))
	}
}

func (overseer *Overseer) handler(w http.ResponseWriter, r *http.Request) {
	conn, _, _, err := ws.UpgradeHTTP(r, w)
	if err != nil {
		logger.Errorw("Failed to establish connection to client socket", "error", err)
	}

	logger.Info("New connection established. Creating new worker ...")
	worker := NewWorker(overseer, conn, "unknown", nil, nil)
	go worker.Start()
}

func (overseer *Overseer) createDir(path string) {
	_, err := os.Stat(filepath.Dir(path))
	if os.IsNotExist(err) {
		err := os.MkdirAll(filepath.Dir(path), os.ModePerm)
		if err != nil {
			logger.Fatalw("Failed to create directory of db path", "path", path, "error", err)
		}
	}
}

func (overseer *Overseer) createNewRoomsDB(path string) {
	roomsDB, err := NewDB(path, overseer.config.DBWaitTimeout, overseer.config.DBStatInterval)
	if err != nil {
		logger.Panicw("Failed to create general database handler", "error", err)
	}

	overseer.roomsDB = &roomsDB
}

func (overseer *Overseer) addRooms(roomConfigs map[string]RoomConfig) {
	rooms := make(map[string]*Room, 0)
	rooms = overseer.addRoomsFromDB(rooms)
	rooms = overseer.addRoomsFromConfig(rooms, roomConfigs)

	overseer.rooms = rooms
}

func (overseer *Overseer) addRoomsFromDB(rooms map[string]*Room) map[string]*Room {
	roomConfigs := overseer.roomsDB.GetRoomConfigs("general")

	for name, roomConfig := range roomConfigs {
		newRoom := NewRoom(name, overseer.config.DBPath, overseer.config.DBUpdateInterval, overseer.config.DBWaitTimeout, overseer.config.DBStatInterval, roomConfig.Persistent)
		rooms[name] = &newRoom
	}

	return rooms
}

func (overseer *Overseer) addRoomsFromConfig(rooms map[string]*Room, roomConfigs map[string]RoomConfig) map[string]*Room {
	for name, roomConfig := range roomConfigs {
		if _, ok := rooms[name]; ok {
			continue
		}

		newRoom := NewRoom(name, overseer.config.DBPath, overseer.config.DBUpdateInterval, overseer.config.DBWaitTimeout, overseer.config.DBStatInterval, roomConfig.Persistent)
		overseer.writeRoom(&newRoom)
		rooms[name] = &newRoom
	}

	return rooms
}

func (overseer *Overseer) createOrFindRoom(roomName string) *Room {
	var newRoom *Room
	if overseer.rooms[roomName] == nil {
		tmpRoom := NewRoom(roomName, overseer.config.DBPath, overseer.config.DBUpdateInterval, overseer.config.DBWaitTimeout, overseer.config.DBStatInterval, false) //new rooms are never persistent
		newRoom = &tmpRoom
		overseer.appendRoom(newRoom)
		overseer.writeRoom(newRoom)
		newRoom.writePlaylist()
	} else {
		newRoom = overseer.rooms[roomName]
	}

	return newRoom
}

func (overseer *Overseer) handleJoin(join *Join, worker *Worker) {
	logger.Debugw("Received login attempt", "message", join)
	if overseer.passwordCheckFailed(join.Password) {
		worker.sendServerMessage("Password is incorrect. Please try again", true)
		return
	}

	if worker.isLoggedIn() {
		overseer.handleRoomChange(join, worker)
	} else {
		overseer.handleFirstLogin(join, worker)
	}
}

func (overseer *Overseer) passwordCheckFailed(password string) bool {
	return overseer.config.Password != "" && password != overseer.config.Password
}

func (overseer *Overseer) handleRoomChange(join *Join, worker *Worker) {
	worker.settings.room.deleteWorker(worker)
	overseer.updateRoomChangeState(join.Room, worker)
	overseer.sendRoomChangeUpdates(worker)
}

func (overseer *Overseer) handleFirstLogin(join *Join, worker *Worker) {
	// it is important to first set the state and then login.
	// Otherwise, messages from the client may be handle with an incorrect state
	overseer.updateRoomChangeState(join.Room, worker)
	worker.login()
	overseer.sendRoomChangeUpdates(worker)
}

func (overseer *Overseer) sendRoomChangeUpdates(worker *Worker) {
	worker.overseer.broadcastStatusList(worker)
	worker.sendPlaylist()
	worker.settings.room.sendSeekWithLock(worker, true)
}

func (overseer *Overseer) updateRoomChangeState(roomName string, worker *Worker) {
	room := overseer.createOrFindRoom(roomName)
	room.appendWorker(worker)
	worker.updateVideoStatus(&VideoStatus{Filename: room.state.video, Position: room.state.position, Paused: room.state.paused}, time.Now())
	worker.updateRoom(room)
}

func (overseer *Overseer) appendRoom(room *Room) {
	overseer.roomsMutex.Lock()
	defer overseer.roomsMutex.Unlock()

	overseer.rooms[room.name] = room
}

func (overseer *Overseer) writeRoom(room *Room) {
	overseer.roomsMutex.RLock()
	defer overseer.roomsMutex.RUnlock()

	//needs to be extended in case more options are added to room, e.g. a room config
	config := RoomConfig{Persistent: room.persistent}
	byteConfig, err := json.Marshal(config)
	if err != nil {
		logger.Warnw("Failed to marshal room config", "error", err)
		return
	}

	overseer.roomsDB.Update("general", room.name, byteConfig)
}

func (overseer *Overseer) deleteRoom(room *Room) {
	overseer.roomsMutex.Lock()
	defer overseer.roomsMutex.Unlock()
	defer room.db.Close()

	delete(overseer.rooms, room.name)
	room.db.DeleteBucket(room.name)
	overseer.deleteRoomFromBucket(room)
	close(room.dbChannel)
}

func (overseer *Overseer) deleteRoomFromBucket(room *Room) {
	overseer.roomsDB.DeleteKey("general", room.name)
}

// TODO tidy
func (overseer *Overseer) statusList() map[string][]Status {
	overseer.roomsMutex.RLock()
	defer overseer.roomsMutex.RUnlock()

	statusList := make(map[string][]Status, 0)
	for _, room := range overseer.rooms {
		statusList[room.name] = overseer.retrieveWorkerStatus(room)
	}

	return statusList
}

func (overseer *Overseer) retrieveWorkerStatus(room *Room) []Status {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	statusList := make([]Status, 0)
	for _, worker := range room.workers {
		worker.userStatusMutex.RLock()
		statusList = append(statusList, *worker.userStatus)
		worker.userStatusMutex.RUnlock()
	}

	return statusList
}

// StatusList is also sent to the client who sent the last VideoStatus
func (overseer *Overseer) broadcastStatusList(worker *Worker) {
	rooms := overseer.statusList()

	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	statusList := StatusList{Rooms: rooms, Username: worker.userStatus.Username}
	message, err := MarshalMessage(statusList)
	if err != nil {
		logger.Errorw("Unable to parse status list", "error", err)
		return
	}

	overseer.broadcastAll(message)
}

func (overseer *Overseer) broadcastAll(message []byte) {
	overseer.roomsMutex.RLock()
	defer overseer.roomsMutex.RUnlock()

	for _, room := range overseer.rooms {
		room.broadcastAll(message)
	}
}
