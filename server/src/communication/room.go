package communication

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"path/filepath"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/sevenautumns/niketsu/server/src/db"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

type RoomStateHandler interface {
	BroadcastAll(message []byte)
	BroadcastStart(worker ClientWorker, all bool)
	BroadcastSeek(filename string, position uint64, worker ClientWorker, desync bool)
	BroadcastSelect(filename *string, worker ClientWorker, all bool)
	BroadcastUserMessage(message string, worker ClientWorker)
	BroadcastPlaylist(playlist Playlist, worker ClientWorker, all bool)
	BroadcastPause(worker ClientWorker)
	BroadcastStartOnReady(worker ClientWorker)
	BroadcastPlaybackSpeed(speed float64, worker ClientWorker)
	SetVideo(fileName *string)
	SetPosition(position uint64)
	SetLastSeek(position uint64)
	SetSpeed(speed float64)
	SetPlaylistState(video *string, position uint64, paused bool, lastSeek uint64)
	SetPlaylist(playlist []string)
	Speed() float64
	DeleteWorker(uuid uuid.UUID)
	AppendWorker(worker ClientWorker)
	SetPaused(paused bool)
	RoomState() *RoomState
	Name() string
	WorkerStatus() []Status
	HandleVideoStatus(worker ClientWorker)
	HandlePlaylistUpdate(playlist []string, worker ClientWorker)
	WritePlaylist()
	IsEmpty() bool
	IsPlaylistEmpty() bool
	IsPersistent() bool
	Close() error
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
func NewRoom(name string, path string, dbUpdateInterval uint64, dbWaitTimeout uint64, persistent bool) Room {
	var room Room
	room.name = name
	room.workers = make([]ClientWorker, 0)
	room.workersMutex = &sync.RWMutex{}
	room.initNewDB(path, dbWaitTimeout)
	room.stateMutex = &sync.RWMutex{}
	room.state = &RoomState{lastSeek: 0, paused: true, speed: 1.0}
	room.setStateFromDB()
	room.dbUpdateInterval = time.Duration(dbUpdateInterval * uint64(time.Second))
	room.dbChannel = make(chan int)
	room.persistent = persistent
	go room.dbIntervalUpdate()

	return room
}

func (room *Room) initNewDB(path string, dbWaitTimeout uint64) {
	dbpath := filepath.Join(path, room.name+".db")

	keyValueStore, err := db.NewBoltKeyValueStore(dbpath, dbWaitTimeout)
	if err != nil {
		logger.Fatalw("Failed to create database for room", "room", room.name, "error", err)
	}
	db := db.NewDBManager(keyValueStore)
	room.db = &db

	err = room.db.Open()
	if err != nil {
		logger.Fatalw("Failed to open database for room", "room", room.name, "error", err)
	}
}

func (room *Room) dbIntervalUpdate() {
	ticker := time.NewTicker(room.dbUpdateInterval)
	defer ticker.Stop()

	for {
		select {
		case <-room.dbChannel:
			return
		case <-ticker.C:
			room.WritePlaylist()
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
		if *otherWorker.GetUUID() == uuid {
			room.workers = append(room.workers[:i], room.workers[i+1:]...)
		}
	}
}

func (room *Room) broadcastExcept(payload []byte, uuid uuid.UUID) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	for _, worker := range room.workers {
		if *worker.GetUUID() != uuid {
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

func (room *Room) BroadcastStart(worker ClientWorker, all bool) {
	userStatus := worker.GetUserStatus()
	start := Start{Username: userStatus.Username}
	payload, err := MarshalMessage(start)
	if err != nil {
		logger.Errorw("Unable to marshal start message", "error", err)
		return
	}

	if all {
		room.BroadcastAll(payload)

	} else {
		uuid := worker.GetUUID()
		room.broadcastExcept(payload, *uuid)
	}
}

func (room *Room) BroadcastSeek(filename string, position uint64, worker ClientWorker, desync bool) {
	userStatus := worker.GetUserStatus()
	seek := Seek{Filename: filename, Position: position, Speed: room.state.speed, Paused: room.state.paused, Desync: desync, Username: userStatus.Username}
	payload, err := MarshalMessage(seek)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast seek", "error", err)
		return
	}

	uuid := worker.GetUUID()
	room.broadcastExcept(payload, *uuid)
}

func (room *Room) BroadcastSelect(filename *string, worker ClientWorker, all bool) {
	userStatus := worker.GetUserStatus()
	sel := Select{Filename: filename, Username: userStatus.Username}
	payload, err := MarshalMessage(sel)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast select", "error", err)
		return
	}

	if all {
		room.BroadcastAll(payload)
	} else {
		uuid := worker.GetUUID()
		room.broadcastExcept(payload, *uuid)
	}
}

func (room *Room) BroadcastUserMessage(message string, worker ClientWorker) {
	userStatus := worker.GetUserStatus()
	userMessage := UserMessage{Message: message, Username: userStatus.Username}
	payload, err := MarshalMessage(userMessage)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast user message", "error", err)
		return
	}

	uuid := worker.GetUUID()
	room.broadcastExcept(payload, *uuid)
}

func (room *Room) BroadcastPlaylist(playlist Playlist, worker ClientWorker, all bool) {
	userStatus := worker.GetUserStatus()
	pl := Playlist{Playlist: playlist.Playlist, Username: userStatus.Username}
	payload, err := MarshalMessage(pl)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playlist", "error", err)
		return
	}

	if all {
		room.BroadcastAll(payload)
	} else {
		uuid := worker.GetUUID()
		room.broadcastExcept(payload, *uuid)
	}
}

func (room *Room) BroadcastPause(worker ClientWorker) {
	userStatus := worker.GetUserStatus()
	pause := Pause{Username: userStatus.Username}
	payload, err := MarshalMessage(pause)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast pause", "error", err)
		return
	}

	uuid := worker.GetUUID()
	room.broadcastExcept(payload, *uuid)
}

// set paused to false since video will start
func (room *Room) BroadcastStartOnReady(worker ClientWorker) {
	// cannot start nil video
	if room.isVideoNil() {
		return
	}

	if room.allUsersReady() {
		userStatus := worker.GetUserStatus()
		start := Start{Username: userStatus.Username}
		payload, err := MarshalMessage(start)
		if err != nil {
			logger.Errorw("Unable to marshal start message", "error", err)
			return
		}

		room.BroadcastAll(payload)
		room.SetPaused(false)
	}
}

func (room *Room) allUsersReady() bool {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	ready := true
	for _, worker := range room.workers {
		userStatus := worker.GetUserStatus()
		ready = ready && userStatus.Ready
	}

	return ready
}

func (room *Room) BroadcastPlaybackSpeed(speed float64, worker ClientWorker) {
	userStatus := worker.GetUserStatus()
	playbackSpeed := PlaybackSpeed{Speed: speed, Username: userStatus.Username}
	payload, err := MarshalMessage(playbackSpeed)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playbackspeed", "error", err)
		return
	}

	uuid := worker.GetUUID()
	room.broadcastExcept(payload, *uuid)
}

// Evaluates the video states of all clients and broadcasts seek if difference between
// fastest and slowest clients is too large. Can not seek before the last seek's position.
func (room *Room) HandleVideoStatus(worker ClientWorker) {
	maxPosition := room.findFastest()
	workerPosition := worker.GetVideoState().position
	room.setNewPosition(*workerPosition)

	if workerPosition == nil || maxPosition-*workerPosition > uint64(float64(maxClientDifferenceMillisecodns)*room.state.speed) {
		worker.SendSeek(true)
	}
}

func (room *Room) findFastest() uint64 {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	maxPosition := uint64(0)

	for _, worker := range room.workers {
		videoStatus := worker.GetVideoState()
		if videoStatus.position == nil {
			continue
		}

		estimatedPosition := worker.EstimatePosition()
		if estimatedPosition > maxPosition {
			maxPosition = estimatedPosition
		}
	}

	return maxPosition
}

func (room *Room) setNewPosition(position uint64) {
	lastSeek := room.RoomState().lastSeek
	if position > lastSeek {
		room.SetPosition(position)
	} else {
		room.SetPosition(lastSeek)
	}
}

func (room *Room) HandlePlaylistUpdate(playlist []string, worker ClientWorker) {
	if !room.isVideoNil() && room.playlistLessElementsThan(playlist) {
		nextVideo := room.findNext(playlist)
		room.setNextVideo(nextVideo, worker)
	}

	room.SetPlaylist(playlist)
}

func (room *Room) isVideoNil() bool {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	return room.state.video == nil
}

func (room *Room) playlistLessElementsThan(playlist []string) bool {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	return len(playlist) != 0 && len(playlist) < len(room.state.playlist)
}

func (room *Room) findNext(newPlaylist []string) string {
	newPlaylistPosition := 0

	for _, video := range room.state.playlist {
		if video == *room.state.video {
			break
		}

		if video == newPlaylist[newPlaylistPosition] {
			newPlaylistPosition += 1
		}

		if newPlaylistPosition >= len(newPlaylist) {
			newPlaylistPosition -= 1
			break
		}
	}

	return newPlaylist[newPlaylistPosition]
}

func (room *Room) setNextVideo(nextVideo string, worker ClientWorker) {
	if nextVideo != *room.state.video {
		room.SetPlaylistState(&nextVideo, 0, true, 0)
		room.BroadcastSelect(room.state.video, worker, true)
	}
}

func (room *Room) SetPlaylistState(video *string, position uint64, paused bool, lastSeek uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.video = video
	room.state.position = &position
	room.state.paused = paused
	room.state.lastSeek = lastSeek
}

func (room *Room) SetVideo(fileName *string) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.video = fileName
}

func (room *Room) SetPosition(position uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.position = &position
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

func (room *Room) Speed() float64 {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	return room.state.speed
}

func (room *Room) SetPaused(paused bool) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.paused = paused
}

func (room *Room) SetPlaylist(playlist []string) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.playlist = playlist
}

// Writes playlist to database. Currently does not lock playlist to ensure that the lock does not block during writing.
func (room *Room) WritePlaylist() {
	bytePlaylist, err := json.Marshal(room.state.playlist)
	if err != nil {
		logger.Warnw("Failed to marshal playlist", "error", err)
		return
	}

	video := ""
	if room.state.video != nil {
		video = *room.state.video
	}

	position := uint64(0)
	if room.state.position != nil {
		position = *room.state.position
	}

	err = room.db.UpdatePlaylist(room.name, bytePlaylist, video, position)
	if err != nil {
		logger.Warnw("Update key/value transaction for playlist failed", "error", err)
	}
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
	values, err := room.getPlaylist()
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
	values, err := room.getVideo()
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
	values, err := room.getPosition()
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

func (room *Room) getPlaylist() ([]byte, error) {
	return room.db.GetValue(room.name, db.PlaylistKey)
}

func (room *Room) getVideo() ([]byte, error) {
	return room.db.GetValue(room.name, db.VideoKey)
}

func (room *Room) getPosition() ([]byte, error) {
	return room.db.GetValue(room.name, db.PositionKey)
}

func (room *Room) Close() error {
	close(room.dbChannel)

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

func (room *Room) deleteDB() error {
	return room.db.DeleteBucket(room.name)
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
		userStatus := worker.GetUserStatus()
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
