package communication

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"path/filepath"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/sevenautumns/niketsu/server/src/db"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

type RoomStateHandler interface {
	AllUsersReady() bool
	AppendWorker(worker ClientWorker)
	BroadcastAll(message []byte)
	BroadcastExcept(payload []byte, uuid uuid.UUID)
	DeleteWorker(uuid uuid.UUID)
	FastestClientPosition() uint64
	SetPosition(position uint64)
	SetLastSeek(position uint64)
	SetSpeed(speed float64)
	SetPlaylistState(video *string, position uint64, paused bool, lastSeek uint64)
	SetPaused(paused bool)
	RoomState() *RoomState
	Name() string
	WorkerStatus() []Status
	IsEmpty() bool
	IsPlaylistEmpty() bool
	IsPersistent() bool
	Close() error
	Start()
}

type RoomState struct {
	playlist []string
	video    *string
	position *uint64
	lastSeek uint64
	paused   bool
	speed    float64
}

type Room struct {
	name             string
	workers          []ClientWorker
	workersMutex     *sync.RWMutex
	state            *RoomState
	stateMutex       *sync.RWMutex
	db               *db.DBManager
	dbUpdateInterval time.Duration
	dbChannel        chan (int)
	persistent       bool
}

// Creates a new Room which handles requests from workers in a shared channel. The database is created in a file at path/name.db
func NewRoom(name string, path string, dbUpdateInterval uint64, dbWaitTimeout uint64, persistent bool) (RoomStateHandler, error) {
	var room Room
	room.name = name
	room.workers = make([]ClientWorker, 0)
	room.workersMutex = &sync.RWMutex{}
	err := room.initDB(path, dbWaitTimeout)
	if err != nil {
		return nil, err
	}
	room.stateMutex = &sync.RWMutex{}
	room.state = &RoomState{lastSeek: 0, paused: true, speed: 1.0}
	room.setStateFromDB()
	room.dbUpdateInterval = time.Duration(dbUpdateInterval * uint64(time.Second))
	room.dbChannel = make(chan int)
	room.persistent = persistent

	return &room, nil
}

func (room *Room) initDB(path string, dbWaitTimeout uint64) error {
	err := CreateDir(path)
	if err != nil {
		return err
	}
	dbpath := filepath.Join(path, room.name+".db")

	keyValueStore, err := db.NewBoltKeyValueStore(dbpath, dbWaitTimeout)
	if err != nil {
		return errors.New("Failed to create database for room")
	}
	db := db.NewDBManager(keyValueStore)
	room.db = &db

	err = room.db.Open()
	if err != nil {
		return errors.New("Failed to open database for room")
	}

	return nil
}

func (room *Room) Start() {
	room.dbChannel = make(chan int)
	ticker := time.NewTicker(room.dbUpdateInterval)
	defer ticker.Stop()

	for {
		select {
		case <-room.dbChannel:
			return
		case <-ticker.C:
			err := room.writePlaylist()
			if err != nil {
				logger.Warnw("Failed to write playlist to db", "error", err)
			}
		}
	}
}

func (room *Room) AppendWorker(worker ClientWorker) {
	room.workersMutex.Lock()
	defer room.workersMutex.Unlock()

	room.workers = append(room.workers, worker)
}

func (room *Room) DeleteWorker(uuid uuid.UUID) {
	room.workersMutex.Lock()
	defer room.workersMutex.Unlock()

	for i, otherWorker := range room.workers {
		if *otherWorker.UUID() == uuid {
			room.workers = append(room.workers[:i], room.workers[i+1:]...)
		}
	}
}

func (room *Room) BroadcastExcept(payload []byte, uuid uuid.UUID) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	for _, worker := range room.workers {
		if *worker.UUID() != uuid {
			worker.SendMessage(payload)
		}
	}
}

func (room *Room) BroadcastAll(payload []byte) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	for _, worker := range room.workers {
		worker.SendMessage(payload)
	}
}

func (room *Room) AllUsersReady() bool {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	ready := true
	for _, worker := range room.workers {
		userStatus := worker.UserStatus()
		ready = ready && userStatus.Ready
	}

	return ready
}

func (room *Room) FastestClientPosition() uint64 {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	maxPosition := uint64(0)

	for _, worker := range room.workers {
		estimatedPosition := worker.EstimatePosition()
		if estimatedPosition == nil {
			continue
		}

		if *estimatedPosition > maxPosition {
			maxPosition = *estimatedPosition
		}
	}

	return maxPosition
}

func (room *Room) SetPlaylistState(video *string, position uint64, paused bool, lastSeek uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.video = video
	room.state.position = &position
	room.state.paused = paused
	room.state.lastSeek = lastSeek
}

func (room *Room) SetPosition(position uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	lastSeek := room.state.lastSeek
	if position > lastSeek {
		room.state.position = &position
	} else {
		room.state.position = &lastSeek
	}
}

func (room *Room) SetLastSeek(position uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.lastSeek = position
}

func (room *Room) SetSpeed(speed float64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.speed = speed
}

func (room *Room) SetPaused(paused bool) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.paused = paused
}

func (room *Room) writePlaylist() error {
	state := room.RoomState()
	bytePlaylist, err := json.Marshal(state.playlist)
	if err != nil {
		return errors.New("Failed to marshal playlist")
	}

	video := ""
	if state.video != nil {
		video = *room.state.video
	}

	position := uint64(0)
	if state.position != nil {
		position = *room.state.position
	}

	err = room.db.UpdatePlaylist(room.name, bytePlaylist, video, position)
	if err != nil {
		return errors.New("Update key/value transaction for playlist failed")
	}

	return nil
}

// Accesses database and gets state. If failed, falls back to default values
func (room *Room) setStateFromDB() {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.setPlaylistFromDB()
	room.setVideoFromDB()
	room.setPositionFromDB()
}

func (room *Room) setPlaylistFromDB() {
	values, err := room.playlist()
	if err != nil {
		logger.Debugw("Failed to retrieve playlist. Setting playlist to default state (empty)", "error", err)
		room.state.playlist = make([]string, 0)
	} else {
		var playlist []string
		err = json.Unmarshal(values, &playlist)
		if err != nil {
			logger.Debugw("Failed to unmarshal playlist. Setting playlist to default state (empty)", "error", err)
			room.state.playlist = make([]string, 0)
		} else {
			room.state.playlist = playlist
		}
	}
}

// Retrieves video from database and updates the state of the room
func (room *Room) setVideoFromDB() {
	values, err := room.video()
	if err != nil {
		logger.Debugw("Failed to retrieve video. Setting video to default state (nil)", "error", err)
		room.state.video = nil
	} else {
		video := string(values)
		room.state.video = &video
	}
}

// Retrieves position from database and updates the state of the room
func (room *Room) setPositionFromDB() {
	values, err := room.position()
	if err != nil {
		logger.Debugw("Failed to retrieve position. Setting position to default state (nil)", "error", err)
		room.state.position = nil
	} else {
		var position uint64
		err := binary.Read(bytes.NewBuffer(values[:]), binary.LittleEndian, &position)
		if err != nil {
			logger.Debugw("Failed to convert position. Setting position to default state (nil)", "error", err)
			room.state.position = nil
		} else {
			room.state.position = &position
		}
	}
}

func (room *Room) playlist() ([]byte, error) {
	return room.db.GetValue(room.name, db.PlaylistKey)
}

func (room *Room) video() ([]byte, error) {
	return room.db.GetValue(room.name, db.VideoKey)
}

func (room *Room) position() ([]byte, error) {
	return room.db.GetValue(room.name, db.PositionKey)
}

func (room *Room) Close() error {
	room.closeChan()

	err := room.deleteDB()
	if err != nil {
		return err
	}

	err = room.closeDB()
	if err != nil {
		return err
	}

	return nil
}

func (room *Room) closeChan() {
	close(room.dbChannel)
}

func (room *Room) deleteDB() error {
	return room.db.Delete()
}

func (room *Room) closeDB() error {
	return room.db.Close()
}

func (room *Room) RoomState() *RoomState {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	return room.state
}

func (room *Room) Name() string {
	return room.name
}

func (room *Room) WorkerStatus() []Status {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	statusList := make([]Status, 0)
	for _, worker := range room.workers {
		userStatus := worker.UserStatus()
		statusList = append(statusList, *userStatus)
	}

	return statusList
}

func (room *Room) IsEmpty() bool {
	room.workersMutex.Lock()
	defer room.workersMutex.Unlock()

	return len(room.workers) == 0
}

func (room *Room) IsPlaylistEmpty() bool {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	return len(room.state.playlist) == 0
}

func (room *Room) IsPersistent() bool {
	return room.persistent
}
