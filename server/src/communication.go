package niketsu_server

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"math"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/gobwas/ws"
	"github.com/gobwas/ws/wsutil"
	"github.com/google/uuid"
)

// TODO handle clients with unstable latency
// TODO fix start on new video, update status of client when select
const (
	WEIGHTING_FACTOR            float64       = 0.85
	TICK_INTERVALS              time.Duration = time.Second
	MAX_DIFFERENCE_MILLISECONDS uint64        = 1e3 // one second
	//UNSTABLE_LATENCY_THRESHOLD  float64       = 2e3
)

type Latency struct {
	rtt        float64
	timestamps map[uuid.UUID]time.Time
}

type Video struct {
	filename  *string
	position  *uint64
	timestamp time.Time
	paused    bool
	speed     float64
}

// TODO add lock for settings, e.g. loggedIn?
type FactoryWorkerSettings struct {
	uuid           uuid.UUID
	conn           net.Conn
	room           *Room
	loggedIn       bool
	serviceChannel chan int
	closeOnce      sync.Once
}

type FactoryWorker struct {
	capitalist       *Capitalist
	settings         *FactoryWorkerSettings
	settingsMutex    *sync.RWMutex
	userStatus       *Status
	userStatusMutex  *sync.RWMutex
	videoStatus      *Video
	videoStatusMutex *sync.RWMutex
	latency          *Latency
	latencyMutex     *sync.RWMutex
}

func NewFactoryWorker(capitalist *Capitalist, conn net.Conn, userName string, filename *string, position *uint64) FactoryWorker {
	var worker FactoryWorker
	worker.capitalist = capitalist
	worker.settings = &FactoryWorkerSettings{uuid: uuid.New(), conn: conn, room: nil, loggedIn: false, serviceChannel: make(chan int)}
	worker.settingsMutex = &sync.RWMutex{}
	worker.userStatus = &Status{Ready: false, Username: userName}
	worker.userStatusMutex = &sync.RWMutex{}
	worker.videoStatus = &Video{filename: filename, position: position, paused: true, speed: 1.0}
	worker.videoStatusMutex = &sync.RWMutex{}
	worker.latency = &Latency{rtt: 0, timestamps: make(map[uuid.UUID]time.Time)}
	worker.latencyMutex = &sync.RWMutex{}
	return worker
}

func (worker *FactoryWorker) close() {
	worker.settings.closeOnce.Do(func() {
		logger.Debugw("Closing connection", "client", worker.settings.uuid)
		close(worker.settings.serviceChannel)

		if worker.settings.room != nil {
			worker.settings.room.deleteWorker(worker)
			worker.settings.room.checkRoomState(worker)
			worker.capitalist.broadcastStatusList(worker)
		}
	})
}

func (worker *FactoryWorker) login() {
	worker.settingsMutex.Lock()
	defer worker.settingsMutex.Unlock()

	worker.settings.loggedIn = true
}

func (worker *FactoryWorker) updateUserStatus(status *Status) {
	worker.userStatusMutex.Lock()
	defer worker.userStatusMutex.Unlock()

	worker.userStatus = status
}

func (worker *FactoryWorker) updateVideoStatus(videoStatus *VideoStatus, arrivalTime time.Time) {
	worker.videoStatusMutex.Lock()
	defer worker.videoStatusMutex.Unlock()

	worker.videoStatus.filename = videoStatus.Filename
	worker.videoStatus.position = videoStatus.Position
	worker.videoStatus.timestamp = arrivalTime.Add(time.Duration(-worker.latency.rtt/2) * time.Nanosecond)
	worker.videoStatus.paused = videoStatus.Paused
}

func (worker *FactoryWorker) updateSpeed(speed float64) {
	worker.videoStatusMutex.Lock()
	defer worker.videoStatusMutex.Unlock()

	worker.videoStatus.speed = speed
}

func (worker *FactoryWorker) updateRtt(uuid uuid.UUID, arrivalTime time.Time) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	// calculate new avg rtt
	newRtt := float64(arrivalTime.Sub(worker.latency.timestamps[uuid]))
	worker.latency.rtt = worker.latency.rtt*WEIGHTING_FACTOR + newRtt*(1-WEIGHTING_FACTOR)

	delete(worker.latency.timestamps, uuid) // TODO check and delete missing pings
}

func (worker *FactoryWorker) updateRoom(room *Room) {
	worker.settingsMutex.Lock()
	defer worker.settingsMutex.Unlock()

	worker.settings.room = room
}

func (worker *FactoryWorker) sendPing() {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	uuid := uuid.New()
	ping := Ping{Uuid: uuid.String()}
	message, err := ping.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to parse ping message", "error", err)
		return
	}

	worker.latencyMutex.Lock()
	worker.latency.timestamps[uuid] = time.Now()
	worker.latencyMutex.Unlock()

	err = wsutil.WriteServerMessage(worker.settings.conn, ws.OpText, message)
	if err != nil {
		logger.Errorw("Unable to send ping message", "error", err)
		worker.close()
	}
}

func (worker *FactoryWorker) sendMessage(message []byte) {
	err := wsutil.WriteServerMessage(worker.settings.conn, ws.OpText, message)
	//TODO handle different errors
	if err != nil {
		logger.Errorw("Unable to send message", "error", err)
		worker.close()
	}
}

func (worker *FactoryWorker) sendServerMessage(message string, isError bool) {
	serverMessage := ServerMessage{Message: message, IsError: isError}
	data, err := serverMessage.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to parse server message", "error", err)
	}

	worker.sendMessage(data)
}

func (worker *FactoryWorker) sendPlaylist() {
	worker.settingsMutex.RLock()
	worker.settings.room.stateMutex.RLock()
	worker.userStatusMutex.RLock()
	defer worker.settingsMutex.RUnlock()
	defer worker.settings.room.stateMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	playlist := Playlist{Playlist: worker.settings.room.state.playlist, Username: worker.userStatus.Username}
	message, err := playlist.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal playlist", "error", err)
		return
	}

	worker.sendMessage(message)
}

func (worker *FactoryWorker) handlePing(ping *Ping, arrivalTime time.Time) {
	uuid, err := uuid.Parse(ping.Uuid)
	if err != nil {
		logger.Errorw("Unable to parse uuid", "error", err)
		return
	}

	worker.updateRtt(uuid, arrivalTime)
}

func (worker *FactoryWorker) handleStatus(status *Status) {
	worker.updateUserStatus(status)
	worker.capitalist.broadcastStatusList(worker)
	worker.settings.room.broadcastStartOnReady(worker)
}

func (worker *FactoryWorker) handleVideoStatus(videoStatus *VideoStatus, arrivalTime time.Time) {
	if videoStatus.Filename == nil || videoStatus.Position == nil {
		worker.settings.room.handleNilStatus(videoStatus, worker)
		return
	}

	legit := worker.settings.room.checkValidVideoStatus(videoStatus, worker)
	if legit {
		worker.updateVideoStatus(videoStatus, arrivalTime)
		worker.settings.room.evaluateVideoStatus(worker)
	} else {
		worker.settings.room.sendSeek(worker, true, true)
	}
}

func (worker *FactoryWorker) handleStart(start *Start) {
	worker.settings.room.setPaused(false)
	worker.settings.room.broadcastStart(worker)
}

func (worker *FactoryWorker) handleSeek(seek *Seek, arrivalTime time.Time) {
	worker.settings.room.changePosition(seek.Position)
	worker.settings.room.updateLastSeek(seek.Position)
	worker.updateVideoStatus(&VideoStatus{Filename: &seek.Filename, Position: &seek.Position, Paused: seek.Paused}, arrivalTime)
	worker.settings.room.broadcastSeek(seek.Filename, seek.Position, worker, false, true)
}

func (worker *FactoryWorker) handleSelect(sel *Select) {
	worker.settings.room.changePlaylistState(sel.Filename, 0, true, 0, true)
	worker.settings.room.broadcastSelect(sel.Filename, worker, false)
	worker.settings.room.broadcastStartOnReady(worker)
}

func (worker *FactoryWorker) handlePlaylist(playlist *Playlist) {
	worker.settings.room.changePlaylist(playlist.Playlist, worker)
	worker.settings.room.broadcastPlaylist(playlist, worker, true)
}

func (worker *FactoryWorker) handlePause(pause *Pause) {
	worker.settings.room.setPaused(true)
	worker.settings.room.broadcastPause(worker)
}

func (worker *FactoryWorker) handlePlaybackSpeed(speed *PlaybackSpeed) {
	worker.updateSpeed(speed.Speed)
	worker.settings.room.updateSpeed(speed.Speed)
	worker.settings.room.broadcastPlaybackSpeed(speed.Speed, worker)
}

func (worker *FactoryWorker) handleMessage(data []byte, arrivalTime time.Time) {
	defer logger.Debugw("Time passed to handle message", "time", time.Now().Sub(arrivalTime))

	msg, err := UnmarshalMessage(data)
	if err != nil {
		logger.Errorw("Unable to unmarshal client message", "error", err)
		return
	}

	worker.settingsMutex.RLock()
	if !worker.settings.loggedIn && msg.Type() != JoinType {
		worker.settingsMutex.RUnlock()
		return
	}
	worker.settingsMutex.RUnlock()

	logger.Debugw("Received message from client", "name", worker.userStatus.Username, "type", msg.Type(), "message", msg)
	switch msg.Type() {
	case PingType:
		msg := msg.(*Ping)
		worker.handlePing(msg, arrivalTime)
	case StatusType:
		msg := msg.(*Status)
		worker.handleStatus(msg)
	case VideoStatusType:
		msg := msg.(*VideoStatus)
		worker.handleVideoStatus(msg, arrivalTime)
	case StartType:
		msg := msg.(*Start)
		worker.handleStart(msg)
	case SeekType:
		msg := msg.(*Seek)
		worker.handleSeek(msg, arrivalTime)
	case SelectType:
		msg := msg.(*Select)
		worker.handleSelect(msg)
	case UserMessageType:
		msg := msg.(*UserMessage)
		worker.settings.room.broadcastUserMessage(msg.Message, worker)
	case PlaylistType:
		msg := msg.(*Playlist)
		worker.handlePlaylist(msg)
	case PauseType:
		msg := msg.(*Pause)
		worker.handlePause(msg)
	case JoinType:
		msg := msg.(*Join)
		worker.capitalist.handleJoin(msg, worker)
	case PlaybackSpeedType:
		msg := msg.(*PlaybackSpeed)
		worker.handlePlaybackSpeed(msg)
	default:
		logger.Warn("Unknown message handling is not supported.")
	}
}

func (worker *FactoryWorker) Start() {
	defer worker.settings.conn.Close()

	// send client current state
	go worker.HandlerService()
	go worker.PingService()

	<-worker.settings.serviceChannel
}

func (worker *FactoryWorker) HandlerService() {
	for {
		select {
		case <-worker.settings.serviceChannel:
			return
		default:
			data, _, err := wsutil.ReadClientData(worker.settings.conn)
			arrivalTime := time.Now()
			if err != nil {
				logger.Errorw("Unable to read from client", "error", err, "worker", worker.settings.uuid)
				worker.close()
				return
			}
			//TODO handle different op code

			go worker.handleMessage(data, arrivalTime)
		}
	}
}

func (worker *FactoryWorker) PingService() {
	ticker := time.NewTicker(TICK_INTERVALS)
	defer ticker.Stop()

	for {
		select {
		case <-worker.settings.serviceChannel:
			return
		case <-ticker.C:
			worker.sendPing()
		}
	}
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
	workers          []*FactoryWorker
	workersMutex     *sync.RWMutex
	state            *RoomState
	stateMutex       *sync.RWMutex
	karen            *Karen
	dbUpdateInterval time.Duration
	dbChannel        chan (int)
}

// Creates a new Room which handles requests from workers in a shared channel. The database is created in a file at path/name.db
func NewRoom(name string, path string, dbUpdateInterval uint64, dbWaitTimeout uint64, dbStatInterval uint64) Room {
	var room Room
	room.name = name
	room.workers = make([]*FactoryWorker, 0)
	room.workersMutex = &sync.RWMutex{}
	dbpath := filepath.Join(path, name+".db")
	karen, err := NewKaren(dbpath, dbUpdateInterval, dbStatInterval)
	if err != nil {
		logger.Fatalw("Failed to create database. Exiting ...", "error", err)
	}
	room.karen = karen
	go room.karen.Monitor()

	room.stateMutex = &sync.RWMutex{}
	room.state = &RoomState{lastSeek: 0, paused: true, speed: 1.0}
	room.getState(true)
	room.dbUpdateInterval = time.Duration(5 * uint64(time.Second))

	return room
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

func (room *Room) appendWorker(worker *FactoryWorker) {
	room.workersMutex.Lock()
	defer room.workersMutex.Unlock()

	room.workers = append(room.workers, worker)
}

func (room *Room) deleteWorker(worker *FactoryWorker) {
	room.workersMutex.Lock()
	defer room.workersMutex.Unlock()

	// search and destroy
	for i, w := range room.workers {
		if w.settings.uuid == worker.settings.uuid {
			room.workers = append(room.workers[:i], room.workers[i+1:]...)
		}
	}
}

func (room *Room) checkRoomState(worker *FactoryWorker) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()
	// pause video if no clients are connected
	if len(room.workers) == 0 {
		room.state.paused = true

		// delete room if no clients are connected and playlist is empty
		if len(room.state.playlist) == 0 {
			worker.capitalist.deleteRoom(room)
		}
	}
}

type CapitalistConfig struct {
	host             string
	port             uint16
	cert             string
	key              string
	password         string
	dbpath           string
	dbUpdateInterval uint64
	dbWaitTimeout    uint64
	dbStatInterval   uint64
}

type Capitalist struct {
	config     *CapitalistConfig
	rooms      map[string]*Room
	roomsMutex *sync.RWMutex
}

func NewCapitalist(config ServerConfig) Capitalist {
	var capitalist Capitalist
	capitalist.config = &CapitalistConfig{host: config.General.Host, port: config.General.Port, cert: config.General.Cert, key: config.General.Key, password: config.General.Password, dbpath: config.General.DBPath, dbUpdateInterval: config.General.DbUpdateInterval, dbWaitTimeout: config.General.DbWaitTimeout, dbStatInterval: config.General.DbStatInterval}
	rooms := make(map[string]*Room, 0)

	_, err := os.Stat(config.General.DBPath)
	if os.IsNotExist(err) {
		err := os.Mkdir(filepath.Dir(config.General.DBPath), 0700)
		if err != nil {
			logger.Fatalw("Failed to create directory of db path", "error", err)
		}
	}

	for name := range config.Rooms {
		newRoom := NewRoom(name, config.General.DBPath, config.General.DbUpdateInterval, config.General.DbWaitTimeout, config.General.DbStatInterval)
		rooms[name] = &newRoom
	}

	capitalist.rooms = rooms
	capitalist.roomsMutex = &sync.RWMutex{}

	return capitalist
}

func (capitalist *Capitalist) handler(w http.ResponseWriter, r *http.Request) {
	conn, _, _, err := ws.UpgradeHTTP(r, w)
	if err != nil {
		logger.Errorw("Failed to establish connection to client socket", "error", err)
	}

	logger.Info("New connection established. Creating new worker ...")
	worker := NewFactoryWorker(capitalist, conn, "unknown", nil, nil)

	logger.Infow("Starting new worker for client", "client", worker.settings.uuid)
	go worker.Start()
}

func (capitalist *Capitalist) Start() {
	hostPort := fmt.Sprintf("%s:%d", capitalist.config.host, capitalist.config.port)
	if capitalist.config.cert == "" || capitalist.config.key == "" {
		logger.Info("Finished initializing manager. Starting http listener ...")
		http.ListenAndServe(hostPort, http.HandlerFunc(capitalist.handler))
	} else {
		logger.Info("Finished initializing manager. Starting tls listener ...")
		http.ListenAndServeTLS(hostPort, capitalist.config.cert, capitalist.config.key, http.HandlerFunc(capitalist.handler))
	}
}

func (capitalist *Capitalist) createOrFindRoom(roomName string) *Room {
	var newRoom *Room
	if capitalist.rooms[roomName] == nil {
		tmpRoom := NewRoom(roomName, capitalist.config.dbpath, capitalist.config.dbUpdateInterval, capitalist.config.dbWaitTimeout, capitalist.config.dbUpdateInterval)
		newRoom = &tmpRoom
		capitalist.appendRoom(newRoom)
		newRoom.writePlaylist()
		go newRoom.startDBIntervalBackup()
	} else {
		newRoom = capitalist.rooms[roomName]
	}

	return newRoom
}

func (capitalist *Capitalist) handleFirstLogin(join *Join, worker *FactoryWorker) {
	room := capitalist.createOrFindRoom(join.Room)
	room.appendWorker(worker)

	worker.userStatusMutex.RLock()
	status := &Status{Ready: false, Username: worker.userStatus.Username}
	worker.userStatusMutex.RUnlock()
	worker.updateVideoStatus(&VideoStatus{Filename: room.state.video, Position: room.state.position, Paused: room.state.paused}, time.Now())
	worker.updateUserStatus(status)
	worker.updateRoom(room)
	worker.login()

	worker.capitalist.broadcastStatusList(worker)
	worker.sendPlaylist()
	room.sendSeek(worker, true, true)
}

func (capitalist *Capitalist) handleRoomChange(join *Join, worker *FactoryWorker) {
	worker.settings.room.deleteWorker(worker)
	room := capitalist.createOrFindRoom(join.Room)
	room.appendWorker(worker)

	worker.updateVideoStatus(&VideoStatus{Filename: room.state.video, Position: room.state.position, Paused: room.state.paused}, time.Now())
	worker.updateRoom(room)
	worker.capitalist.broadcastStatusList(worker)
	worker.sendPlaylist()
	room.sendSeek(worker, true, true)
}

func (capitalist *Capitalist) handleJoin(join *Join, worker *FactoryWorker) {
	logger.Debugw("Received login attempt", "message", join)
	if capitalist.config.password != "" && join.Password != capitalist.config.password {
		worker.sendServerMessage("Password is incorrect. Please try again", true)
		return
	}

	worker.settingsMutex.RLock()
	loggedIn := worker.settings.loggedIn
	worker.settingsMutex.RUnlock()

	if loggedIn {
		capitalist.handleRoomChange(join, worker)
	} else {
		capitalist.handleFirstLogin(join, worker)
	}
}

func (capitalist *Capitalist) appendRoom(room *Room) {
	capitalist.roomsMutex.Lock()
	defer capitalist.roomsMutex.Unlock()

	capitalist.rooms[room.name] = room
}

func (capitalist *Capitalist) deleteRoom(room *Room) {
	capitalist.roomsMutex.Lock()
	defer capitalist.roomsMutex.Unlock()
	defer room.karen.Close()

	delete(capitalist.rooms, room.name)
	room.karen.DeleteBucket(room.name)
}

func (capitalist *Capitalist) statusList() map[string][]Status {
	capitalist.roomsMutex.RLock()
	defer capitalist.roomsMutex.RUnlock()

	statusList := make(map[string][]Status, 0)
	for _, room := range capitalist.rooms {
		room.workersMutex.RLock()
		statusList[room.name] = make([]Status, 0)
		for _, worker := range room.workers {
			worker.userStatusMutex.RLock()
			statusList[room.name] = append(statusList[room.name], *worker.userStatus)
			worker.userStatusMutex.RUnlock()
		}
		room.workersMutex.RUnlock()
	}

	return statusList
}

// StatusList is also sent to the client who sent the last VideoStatus
func (capitalist *Capitalist) broadcastStatusList(worker *FactoryWorker) {
	rooms := capitalist.statusList()

	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	statusList := StatusList{Rooms: rooms, Username: worker.userStatus.Username}
	message, err := statusList.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to parse status list", "error", err)
		return
	}

	capitalist.broadcastAll(message)
}

func (capitalist *Capitalist) broadcastAll(message []byte) {
	capitalist.roomsMutex.RLock()
	defer capitalist.roomsMutex.RUnlock()

	for _, room := range capitalist.rooms {
		room.broadcastAll(message)
	}
}

func (room *Room) broadcastExcept(message []byte, worker *FactoryWorker) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	for _, w := range room.workers {
		if w.settings.uuid != worker.settings.uuid {
			w.sendMessage(message)
		}
	}
}

func (room *Room) broadcastAll(message []byte) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	for _, w := range room.workers {
		w.sendMessage(message)
	}
}

func (room *Room) broadcastStart(worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Start{Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal start message", "error", err)
		return
	}

	room.broadcastExcept(message, worker)
}

func (room *Room) broadcastSeek(filename string, position uint64, worker *FactoryWorker, desync bool, lock bool) {
	if lock {
		room.stateMutex.RLock()
		defer room.stateMutex.RUnlock()
	}

	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Seek{Filename: filename, Position: position, Speed: room.state.speed, Paused: room.state.paused, Desync: desync, Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast seek", "error", err)
		return
	}

	room.broadcastExcept(message, worker)
}

func (room *Room) broadcastSelect(filename *string, worker *FactoryWorker, all bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Select{Filename: filename, Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast select", "error", err)
		return
	}

	if all {
		room.broadcastAll(message)
	} else {
		room.broadcastExcept(message, worker)
	}
}

func (room *Room) broadcastUserMessage(userMessage string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	um := UserMessage{Message: userMessage, Username: worker.userStatus.Username}
	message, err := um.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast user message", "error", err)
		return
	}

	room.broadcastExcept(message, worker)
}

func (room *Room) broadcastPlaylist(playlist *Playlist, worker *FactoryWorker, all bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	pl := Playlist{Playlist: playlist.Playlist, Username: worker.userStatus.Username}
	message, err := pl.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playlist", "error", err)
		return
	}

	if all {
		room.broadcastAll(message)
	} else {
		room.broadcastExcept(message, worker)
	}
}

func (room *Room) broadcastPause(worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	p := Pause{Username: worker.userStatus.Username}
	message, err := p.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast pause", "error", err)
		return
	}

	room.broadcastExcept(message, worker)
}

// set paused to false since video will start
func (room *Room) broadcastStartOnReady(worker *FactoryWorker) {
	room.workersMutex.RLock()
	defer room.workersMutex.RUnlock()

	// cannot start nil video
	if room.state.video == nil {
		return
	}

	ready := true
	for _, w := range room.workers {
		w.userStatusMutex.RLock()
		ready = ready && w.userStatus.Ready
		w.userStatusMutex.RUnlock()
	}

	if ready {
		room.stateMutex.Lock()
		worker.userStatusMutex.RLock()
		defer room.stateMutex.Unlock()
		defer worker.userStatusMutex.RUnlock()

		start := Start{Username: worker.userStatus.Username}
		message, err := start.MarshalMessage()
		if err != nil {
			logger.Errorw("Unable to marshal start message", "error", err)
			return
		}

		for _, w := range room.workers {
			w.sendMessage(message)
		}

		room.state.paused = false
	}
}

func (room *Room) broadcastPlaybackSpeed(speed float64, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	pl := PlaybackSpeed{Speed: speed, Username: worker.userStatus.Username}
	message, err := pl.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playbackspeed", "error", err)
		return
	}

	room.broadcastExcept(message, worker)
}

func (room *Room) sendSeek(worker *FactoryWorker, desync bool, lock bool) {
	if lock {
		room.stateMutex.RLock()
		worker.userStatusMutex.RLock()
		defer room.stateMutex.RUnlock()
		defer worker.userStatusMutex.RUnlock()
	}

	// seeking nil videos is prohibited
	// may need to be changed to allow synchronization even if playlist is empty
	if room.state.video == nil {
		return
	}

	// add half rtt if video is playing
	position := *room.state.position
	if !worker.videoStatus.paused {
		position += uint64(worker.latency.rtt / float64(time.Millisecond) / 2)
	}

	seek := Seek{Filename: *room.state.video, Position: position, Speed: room.state.speed, Paused: room.state.paused, Desync: desync, Username: worker.userStatus.Username}
	message, err := seek.MarshalMessage()
	if err != nil {
		logger.Errorw("Capitalist failed to marshal seek", "error", err)
		return
	}

	worker.sendMessage(message)
}

// Evaluates the video states of all clients and broadcasts seek if difference between
// fastest and slowest clients is too large.
func (room *Room) evaluateVideoStatus(worker *FactoryWorker) {
	room.workersMutex.RLock()
	room.stateMutex.Lock()
	defer room.workersMutex.RUnlock()
	defer room.stateMutex.Unlock()

	minPosition := uint64(math.MaxUint64)
	maxPosition := uint64(0)

	for _, w := range room.workers {
		w.videoStatusMutex.RLock()

		if w.videoStatus.position == nil {
			w.videoStatusMutex.RUnlock()
			continue
		}

		// estimate position of client based on previous position, time difference and playback speed
		// if video is paused, position should remain the same
		var estimatedPosition uint64
		if w.videoStatus.paused {
			estimatedPosition = *w.videoStatus.position
		} else {
			timeElapsed := uint64(float64(time.Since(w.videoStatus.timestamp).Milliseconds()) * room.state.speed)
			estimatedPosition = *w.videoStatus.position + timeElapsed
		}

		if estimatedPosition < minPosition {
			minPosition = estimatedPosition
		}

		if estimatedPosition > maxPosition {
			maxPosition = estimatedPosition
		}

		w.videoStatusMutex.RUnlock()
	}

	// position can not be before lastSeek
	if minPosition > room.state.lastSeek {
		room.state.position = &minPosition
	} else {
		room.state.position = &room.state.lastSeek
	}

	// if difference is too large, all clients are reset based on the slowest client
	if maxPosition-minPosition > uint64(float64(MAX_DIFFERENCE_MILLISECONDS)*room.state.speed) {
		room.sendSeek(worker, true, false)
	}

	go room.writePlaylist()
}

func (room *Room) findNext(newPlaylist []string) string {
	j := 0

	for _, video := range room.state.playlist {
		if video == *room.state.video {
			break
		}

		if video == newPlaylist[j] {
			j += 1
		}

		if j >= len(newPlaylist) {
			j -= 1
			break
		}
	}

	return newPlaylist[j]
}

func (room *Room) changePlaylistState(video *string, position uint64, paused bool, lastSeek uint64, lock bool) {
	if lock {
		room.stateMutex.Lock()
		defer room.stateMutex.Unlock()
	}

	room.state.video = video
	room.state.position = &position
	room.state.paused = paused
	room.state.lastSeek = lastSeek
}

func (room *Room) changePlaylist(playlist []string, worker *FactoryWorker) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	if len(playlist) != 0 && len(playlist) < len(room.state.playlist) {
		nextVideo := room.findNext(playlist)
		if nextVideo != *room.state.video {
			room.changePlaylistState(&nextVideo, 0, true, 0, false)
			room.broadcastSelect(room.state.video, worker, true)
		}
	}

	room.state.playlist = playlist

	go room.writePlaylist()
}

func (room *Room) changeVideo(fileName *string) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.video = fileName
	go room.writePlaylist()
}

func (room *Room) changePosition(position uint64) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.state.position = &position
	go room.writePlaylist()
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

func (room *Room) checkValidVideoStatus(videoStatus *VideoStatus, worker *FactoryWorker) bool {
	room.stateMutex.RLock()
	defer room.stateMutex.RUnlock()

	// video status is not compatible with server if position is not in accordance with the last seek or video is paused when it is not supposed to be
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

func (room *Room) handleNilStatus(videoStatus *VideoStatus, worker *FactoryWorker) {
	room.stateMutex.RLock()
	worker.userStatusMutex.RLock()
	defer room.stateMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	if videoStatus.Filename != room.state.video || videoStatus.Position != room.state.position {
		room.sendSeek(worker, false, false)
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

	room.karen.Update(room.name, bytePlaylist, video, position)
}

// please do not touch my spaghetti
// Retrieves playlist from database and updates the state of the room
func (room *Room) getPlaylist(useDefault bool) {
	values, err := room.karen.Get(room.name, "playlist")
	if err != nil {
		logger.Debugw("Failed to retrieve playlist", "error", err)
		if useDefault {
			logger.Debug("Setting playlist to default state (empty)")
			room.state.playlist = make([]string, 0)
		}
	} else {
		var playlist []string
		err = json.Unmarshal(values, &playlist)
		if err != nil {
			logger.Debugw("Failed to unmarshal playlist", "error", err)
			if useDefault {
				logger.Debug("Setting playlist to default state (empty)")
				room.state.playlist = make([]string, 0)
			}
		} else {
			room.state.playlist = playlist
		}
	}
}

// Retrieves video from database and updates the state of the room
func (room *Room) getVideo(useDefault bool) {
	values, err := room.karen.Get(room.name, "video")
	if err != nil {
		logger.Debugw("Failed to retrieve video", "error", err)
		if useDefault {
			logger.Debug("Setting video to default state (nil)")
			room.state.video = nil
		}
	} else {
		video := string(values)
		room.state.video = &video
	}
}

// Retrieves position from database and updates the state of the room
func (room *Room) getPosition(useDefault bool) {
	values, err := room.karen.Get(room.name, "position")
	if err != nil {
		logger.Debugw("Failed to retrieve position", "error", err)
		if useDefault {
			logger.Debug("Setting position to default state (nil)")
			room.state.position = nil
		}
	} else {
		var position uint64
		err := binary.Read(bytes.NewBuffer(values[:]), binary.LittleEndian, &position)
		if err != nil {
			logger.Debugw("Failed to convert position", "error", err)
			if useDefault {
				logger.Debug("Setting position to default state (nil)")
				room.state.position = nil
			}
		} else {
			room.state.position = &position
		}
	}
}

// Accesses database and gets state. If failed, falls back to default values if useDefault is set.
func (room *Room) getState(useDefault bool) {
	room.stateMutex.Lock()
	defer room.stateMutex.Unlock()

	room.getPlaylist(useDefault)
	room.getVideo(useDefault)
	room.getPosition(useDefault)
}
