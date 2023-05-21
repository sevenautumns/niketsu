package niketsu_server

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"math"
	"path/filepath"
	"sync"
	"time"
)

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
	workers          []*Worker
	workersMutex     *sync.RWMutex
	state            *RoomState
	stateMutex       *sync.RWMutex
	db               *DB
	dbUpdateInterval time.Duration
	dbChannel        chan (int)
	persistent       bool
}

// Creates a new Room which handles requests from workers in a shared channel. The database is created in a file at path/name.db
func NewRoom(name string, path string, dbUpdateInterval uint64, dbWaitTimeout uint64, dbStatInterval uint64, persistent bool) Room {
	var room Room
	room.name = name
	room.workers = make([]*Worker, 0)
	room.workersMutex = &sync.RWMutex{}
	room.createNewDB(path, dbWaitTimeout, dbStatInterval)
	room.stateMutex = &sync.RWMutex{}
	room.state = &RoomState{lastSeek: 0, paused: true, speed: 1.0}
	room.setStateFromDB()
	room.dbUpdateInterval = time.Duration(dbUpdateInterval * uint64(time.Second))
	room.dbChannel = make(chan int)
	room.persistent = persistent
	go room.db.Monitor()
	go room.startDBIntervalBackup()

	return room
}

func (room *Room) createNewDB(path string, dbWaitTimeout uint64, dbStatInterval uint64) {
	dbpath := filepath.Join(path, room.name+".db")
	db, err := NewDB(dbpath, dbWaitTimeout, dbStatInterval)
	if err != nil {
		logger.Fatalw("Failed to create database", "error", err)
	}

	room.db = &db
}

func (room *Room) startDBIntervalBackup() {
	ticker := time.NewTicker(room.dbUpdateInterval)
	defer ticker.Stop()

	for {
		select {
		case <-room.dbChannel:
			return
		case <-ticker.C:
			room.writePlaylist()
		}
	}
}

func (room *Room) appendWorker(worker *Worker) {
	room.workersMutex.Lock()
	defer room.workersMutex.Unlock()

	room.workers = append(room.workers, worker)
}

func (room *Room) deleteWorker(worker *Worker) {
	room.workersMutex.Lock()
	defer room.workersMutex.Unlock()

	for i, otherWorker := range room.workers {
		if otherWorker.settings.uuid == worker.settings.uuid {
			room.workers = append(room.workers[:i], room.workers[i+1:]...)
		}
	}
}

func (room *Room) checkRoomState(worker *Worker) {
	room.workersMutex.RLock()
	room.stateMutex.Lock()
	defer room.workersMutex.RUnlock()
	defer room.stateMutex.Unlock()

	if len(room.workers) == 0 {
		room.handleEmptyRoom(worker)
	}
}

func (room *Room) handleEmptyRoom(worker *Worker) {
	room.state.paused = true

	if len(room.state.playlist) == 0 && !room.persistent {
		worker.overseer.deleteRoom(room)
	}
}

func (room *Room) broadcastExcept(payload []byte, worker *Worker) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	for _, w := range room.workers {
		if w.settings.uuid != worker.settings.uuid {
			w.sendMessage(payload)
		}
	}
}

func (room *Room) broadcastAll(payload []byte) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	for _, w := range room.workers {
		w.sendMessage(payload)
	}
}

func (room *Room) broadcastStart(worker *Worker, all bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	start := Start{Username: worker.userStatus.Username}
	payload, err := MarshalMessage(start)
	if err != nil {
		logger.Errorw("Unable to marshal start message", "error", err)
		return
	}

	if all {
		room.broadcastAll(payload)

	} else {
		room.broadcastExcept(payload, worker)
	}
}

func (room *Room) broadcastSeek(filename string, position uint64, worker *Worker, desync bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	seek := Seek{Filename: filename, Position: position, Speed: room.state.speed, Paused: room.state.paused, Desync: desync, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(seek)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast seek", "error", err)
		return
	}

	room.broadcastExcept(payload, worker)
}

func (room *Room) broadcastSelect(filename *string, worker *Worker, all bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	sel := Select{Filename: filename, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(sel)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast select", "error", err)
		return
	}

	if all {
		room.broadcastAll(payload)
	} else {
		room.broadcastExcept(payload, worker)
	}
}

func (room *Room) broadcastUserMessage(message string, worker *Worker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	userMessage := UserMessage{Message: message, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(userMessage)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast user message", "error", err)
		return
	}

	room.broadcastExcept(payload, worker)
}

func (room *Room) broadcastPlaylist(playlist *Playlist, worker *Worker, all bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	pl := Playlist{Playlist: playlist.Playlist, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(pl)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playlist", "error", err)
		return
	}

	if all {
		room.broadcastAll(payload)
	} else {
		room.broadcastExcept(payload, worker)
	}
}

func (room *Room) broadcastPause(worker *Worker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	pause := Pause{Username: worker.userStatus.Username}
	payload, err := MarshalMessage(pause)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast pause", "error", err)
		return
	}

	room.broadcastExcept(payload, worker)
}

// set paused to false since video will start
func (room *Room) broadcastStartOnReady(worker *Worker) {
	room.stateMutex.RLock()
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()
	defer room.stateMutex.RUnlock()

	// cannot start nil video
	if room.state.video == nil {
		return
	}

	if room.allUsersReady() {
		worker.userStatusMutex.RLock()
		defer worker.userStatusMutex.RUnlock()

		start := Start{Username: worker.userStatus.Username}
		payload, err := MarshalMessage(start)
		if err != nil {
			logger.Errorw("Unable to marshal start message", "error", err)
			return
		}

		for _, w := range room.workers {
			w.sendMessage(payload)
		}

		room.state.paused = false
	}
}

func (room *Room) allUsersReady() bool {
	ready := true
	for _, w := range room.workers {
		w.userStatusMutex.RLock()
		ready = ready && w.userStatus.Ready
		w.userStatusMutex.RUnlock()
	}

	return ready
}

func (room *Room) broadcastPlaybackSpeed(speed float64, worker *Worker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	playbackSpeed := PlaybackSpeed{Speed: speed, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(playbackSpeed)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playbackspeed", "error", err)
		return
	}

	room.broadcastExcept(payload, worker)
}

func (room *Room) sendSeekWithLock(worker *Worker, desync bool) {
	room.stateMutex.RLock()
	worker.userStatusMutex.RLock()
	defer room.stateMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	room.sendSeek(worker, desync)
}

func (room *Room) sendSeek(worker *Worker, desync bool) {
	// seeking nil videos is prohibited
	// may need to be changed to allow synchronization even if playlist is empty
	if room.state.video == nil || room.state.position == nil {
		return
	}

	// add half rtt if video is playing
	position := *room.state.position
	if !worker.videoStatus.paused {
		position += uint64(worker.latency.roundTripTime / float64(time.Millisecond) / 2)
	}

	seek := Seek{Filename: *room.state.video, Position: position, Speed: room.state.speed, Paused: room.state.paused, Desync: desync, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(seek)
	if err != nil {
		logger.Errorw("Capitalist failed to marshal seek", "error", err)
		return
	}

	worker.sendMessage(payload)
}

// Evaluates the video states of all clients and broadcasts seek if difference between
// fastest and slowest clients is too large. Can not seek before the last seek's position.
func (room *Room) handleVideoStatus(worker *Worker) {
	room.workersMutex.RLock()
	room.stateMutex.Lock()
	defer room.workersMutex.RUnlock()
	defer room.stateMutex.Unlock()

	minPosition, maxPosition := room.findSlowestAndFastest()

	// position can not be before lastSeek
	if minPosition > room.state.lastSeek {
		room.state.position = &minPosition
	} else {
		room.state.position = &room.state.lastSeek
	}

	if maxPosition-minPosition > uint64(float64(MAX_CLIENT_DIFFERENCE_MILLISECONDS)*room.state.speed) {
		room.sendSeek(worker, true)
	}
}

func (room *Room) findSlowestAndFastest() (uint64, uint64) {
	minPosition := uint64(math.MaxUint64)
	maxPosition := uint64(0)

	for _, worker := range room.workers {
		worker.videoStatusMutex.RLock()

		if worker.videoStatus.position == nil {
			worker.videoStatusMutex.RUnlock()
			continue
		}
		estimatedPosition := room.estimateClientPosition(worker)

		if estimatedPosition < minPosition {
			minPosition = estimatedPosition
		}

		if estimatedPosition > maxPosition {
			maxPosition = estimatedPosition
		}

		worker.videoStatusMutex.RUnlock()
	}

	return minPosition, maxPosition
}

func (room *Room) estimateClientPosition(worker *Worker) uint64 {
	var estimatedPosition uint64
	if worker.videoStatus.paused {
		estimatedPosition = *worker.videoStatus.position
	} else {
		timeElapsed := uint64(float64(time.Since(worker.videoStatus.timestamp).Milliseconds()) * room.state.speed)
		estimatedPosition = *worker.videoStatus.position + timeElapsed
	}

	return estimatedPosition
}

func (room *Room) changePlaylist(playlist []string, worker *Worker) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	if len(playlist) != 0 && len(playlist) < len(room.state.playlist) {
		nextVideo := room.findNext(playlist)
		room.selectNext(nextVideo, worker)
	}

	room.state.playlist = playlist
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

func (room *Room) selectNext(nextVideo string, worker *Worker) {
	if nextVideo != *room.state.video {
		room.changePlaylistState(&nextVideo, 0, true, 0)
		room.broadcastSelect(room.state.video, worker, true)
	}
}

func (room *Room) changePlaylistState(video *string, position uint64, paused bool, lastSeek uint64) {
	room.state.video = video
	room.state.position = &position
	room.state.paused = paused
	room.state.lastSeek = lastSeek
}

func (room *Room) changePlaylistStateWithLock(video *string, position uint64, paused bool, lastSeek uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.changePlaylistState(video, position, paused, lastSeek)
}

func (room *Room) changeVideo(fileName *string) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.video = fileName
}

func (room *Room) changePosition(position uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.position = &position
}

func (room *Room) updateLastSeek(position uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.lastSeek = position
}

func (room *Room) updateSpeed(speed float64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.speed = speed
}

func (room *Room) isValidVideoStatus(videoStatus *VideoStatus, worker *Worker) bool {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	// video status is not compatible with server if position is not in accordance with the last seek or video
	// is paused when it is not supposed to be
	if *videoStatus.Position < room.state.lastSeek || videoStatus.Paused != room.state.paused {
		return false
	}

	return true
}

func (room *Room) setPaused(paused bool) {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	room.state.paused = paused
}

// TODO rewrite
func (room *Room) handleNilStatus(videoStatus *VideoStatus, worker *Worker) {
	room.stateMutex.RLock()
	worker.userStatusMutex.RLock()
	defer room.stateMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	if videoStatus.Filename != room.state.video || videoStatus.Position != room.state.position {
		room.sendSeek(worker, false)
	}
}

// Writes playlist to database. Currently does not lock playlist to ensure that the lock does not block during writing.
func (room *Room) writePlaylist() {
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

	room.db.UpdatePlaylist(room.name, bytePlaylist, video, position)
}

// Accesses database and gets state. If failed, falls back to default values
func (room *Room) setStateFromDB() {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.setPlaylist()
	room.setVideo()
	room.setPosition()
}

func (room *Room) setPlaylist() {
	values, err := room.db.Get(room.name, "playlist")
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
func (room *Room) setVideo() {
	values, err := room.db.Get(room.name, "video")
	if err != nil {
		logger.Debugw("Failed to retrieve video. Setting video to default state (nil)", "error", err)
		room.state.video = nil
	} else {
		video := string(values)
		room.state.video = &video
	}
}

// Retrieves position from database and updates the state of the room
func (room *Room) setPosition() {
	values, err := room.db.Get(room.name, "position")
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
