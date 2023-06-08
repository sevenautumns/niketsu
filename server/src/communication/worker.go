package communication

import (
	"fmt"
	"sync"
	"sync/atomic"
	"time"

	"github.com/google/uuid"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

// TODO handle clients with unstable latency
// TODO keep status list up to date on room to reduce acquisition
const (
	latencyWeighingFactor           float64       = 0.85
	pingTickInterval                time.Duration = time.Second
	pingDeleteInterval              time.Duration = 600 * time.Second
	maxClientDifferenceMillisecodns uint64        = 1e3 // one second
	//unstableLatencyThreshold  float64       = 2e3
)

type ClientWorker interface {
	GetUUID() *uuid.UUID
	SetUserStatus(status Status)
	GetUserStatus() *Status
	GetVideoState() *videoState
	SetVideoState(videoStatus VideoStatus, arrivalTime time.Time)
	Login()
	IsLoggedIn() bool
	SendMessage(payload []byte)
	SendServerMessage(message string, isError bool)
	SendSeek(desync bool)
	SendPlaylist()
	EstimatePosition() uint64
	DeleteWorkerFromRoom()
	SetRoom(room RoomStateHandler)
	Close()
	Start()
}

type latency struct {
	roundTripTime float64
	timestamps    map[uuid.UUID]time.Time
}

type videoState struct {
	video     *string
	position  *uint64
	timestamp time.Time
	paused    bool
	speed     float64
}

type workerState struct {
	uuid        *atomic.Pointer[uuid.UUID]
	room        *atomic.Value
	loggedIn    *atomic.Bool
	stopRequest chan int
	closeOnce   sync.Once
}

type Worker struct {
	roomHandler     ServerStateHandler
	webSocket       WebSocket
	state           workerState
	userStatus      *atomic.Pointer[Status]
	videoState      *videoState
	videoStateMutex *sync.RWMutex
	latency         *latency
	latencyMutex    *sync.RWMutex
}

func NewWorker(roomHandler ServerStateHandler, webSocket WebSocket, userName string, filename *string, position *uint64) Worker {
	var worker Worker
	worker.roomHandler = roomHandler
	worker.webSocket = webSocket

	atomicUUID := atomic.Pointer[uuid.UUID]{}
	newUUID := uuid.New()
	atomicUUID.Store(&newUUID)
	atomicBool := atomic.Bool{}
	atomicBool.Store(false)
	worker.state = workerState{uuid: &atomicUUID, room: &atomic.Value{}, loggedIn: &atomicBool, stopRequest: make(chan int)}

	atomicStatus := atomic.Pointer[Status]{}
	atomicStatus.Store(&Status{Ready: false, Username: userName})
	worker.userStatus = &atomicStatus

	worker.videoState = &videoState{video: filename, position: position, paused: true, speed: 1.0}
	worker.videoStateMutex = &sync.RWMutex{}

	worker.latency = &latency{roundTripTime: 0, timestamps: make(map[uuid.UUID]time.Time)}
	worker.latencyMutex = &sync.RWMutex{}

	return worker
}

func (worker *Worker) GetUUID() *uuid.UUID {
	return worker.state.uuid.Load()
}

func (worker *Worker) SetUserStatus(status Status) {
	worker.userStatus.Store(&status)
}

func (worker *Worker) GetUserStatus() *Status {
	return worker.userStatus.Load()
}

func (worker *Worker) GetVideoState() *videoState {
	worker.videoStateMutex.RLock()
	defer worker.videoStateMutex.RUnlock()

	return worker.videoState
}

func (worker *Worker) Login() {
	worker.state.loggedIn.Store(true)
}

func (worker *Worker) IsLoggedIn() bool {
	return worker.state.loggedIn.Load()
}

func (worker *Worker) Close() {
	worker.state.closeOnce.Do(func() {
		close(worker.state.stopRequest)

		if worker.state.room != nil {
			worker.closingCleanup()
		}
	})
}

func (worker *Worker) Start() {
	defer worker.webSocket.Close()

	go worker.handleRequests()
	go worker.generatePings()
	<-worker.state.stopRequest
}

func (worker *Worker) generatePings() {
	pingTimer := worker.schedule(worker.sendPing, pingTickInterval)
	pingDeleteTimer := worker.schedule(worker.deletePings, pingDeleteInterval)
	defer pingTimer.Stop()
	defer pingDeleteTimer.Stop()

	<-worker.state.stopRequest
}

func (worker *Worker) schedule(f func(), interval time.Duration) *time.Ticker {
	ticker := time.NewTicker(interval)
	go func() {
		for {
			select {
			case <-ticker.C:
				f()
			case <-worker.state.stopRequest:
				return
			}
		}
	}()
	return ticker
}

func (worker *Worker) sendPing() {
	uuid := uuid.New()
	payload, err := worker.generateNewPing(uuid)
	if err != nil {
		logger.Errorw("Unable to parse ping message", "error", err)
		return
	}
	worker.addPingEntry(uuid)

	err = worker.webSocket.WriteMessage(payload)
	if err != nil {
		logger.Errorw("Unable to send ping message", "error", err)
		worker.Close()
	}
}

func (worker *Worker) generateNewPing(uuid uuid.UUID) ([]byte, error) {
	ping := Ping{Uuid: uuid.String()}
	payload, err := MarshalMessage(ping)
	if err != nil {
		return nil, err
	}

	return payload, nil
}

func (worker *Worker) addPingEntry(uuid uuid.UUID) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	worker.latency.timestamps[uuid] = time.Now()
}

func (worker *Worker) deletePings() {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	for uuid, timestamp := range worker.latency.timestamps {
		elapsedTime := time.Since(timestamp)
		if elapsedTime > time.Minute {
			delete(worker.latency.timestamps, uuid)
		}
	}
}

func (worker *Worker) handleRequests() {
	for {
		select {
		case <-worker.state.stopRequest:
			return
		default:
			worker.handleData()
		}
	}
}

func (worker *Worker) handleData() {
	data, op, err := worker.webSocket.ReadMessage()
	arrivalTime := time.Now()

	if err != nil {
		logger.Infow("Unable to read from client. Closing connection", "error", err, "worker", worker.state.uuid)
		worker.Close()
		return
	}

	//TODO handle different op code
	if op.IsClose() {
		logger.Infow("Client closed connection")
		worker.Close()
		return
	}

	go worker.handleMessage(data, arrivalTime)
}

func (worker *Worker) handleMessage(data []byte, arrivalTime time.Time) {
	message, err := UnmarshalMessage(data)
	if err != nil {
		logger.Errorw("Unable to unmarshal client message", "error", err)
		return
	}
	if message == nil {
		logger.Warnw("Unable to parse nil message")
		return
	}

	//logger.Debugw("Received message from client", "name", worker.GetUserStatus().Username, "type", message.Type(), "message", message)
	//defer logger.Debugw("Time passed to handle message", "type", message.Type(), "time", time.Now().Sub(arrivalTime))

	if worker.ignoreNotLoggedIn(message) {
		return
	}

	worker.handleMessageTypes(message, arrivalTime)
}

func (worker *Worker) ignoreNotLoggedIn(message Message) bool {
	if !worker.IsLoggedIn() && message.Type() != JoinType {
		return true
	}

	return false
}

func (worker *Worker) handleMessageTypes(message Message, arrivalTime time.Time) {
	switch message.Type() {
	case PingType:
		message := message.(*Ping)
		worker.handlePing(*message, arrivalTime)
	case StatusType:
		message := message.(*Status)
		worker.handleStatus(*message)
	case VideoStatusType:
		message := message.(*VideoStatus)
		worker.handleVideoStatus(*message, arrivalTime)
	case StartType:
		message := message.(*Start)
		worker.handleStart(*message)
	case SeekType:
		message := message.(*Seek)
		worker.handleSeek(*message, arrivalTime)
	case SelectType:
		message := message.(*Select)
		worker.handleSelect(*message)
	case UserMessageType:
		message := message.(*UserMessage)
		room := worker.Room()
		room.BroadcastUserMessage(message.Message, worker)
	case PlaylistType:
		message := message.(*Playlist)
		worker.handlePlaylist(*message)
	case PauseType:
		message := message.(*Pause)
		worker.handlePause(*message)
	case JoinType:
		message := *message.(*Join)
		worker.roomHandler.HandleJoin(message, worker)
	case PlaybackSpeedType:
		message := message.(*PlaybackSpeed)
		worker.handlePlaybackSpeed(*message)
	default:
		serverMessage := fmt.Sprintf("Requested command %s not supported:", message.Type())
		worker.SendServerMessage(serverMessage, true)
	}
}

func (worker *Worker) Room() RoomStateHandler {
	return worker.state.room.Load().(RoomStateHandler)
}

func (worker *Worker) SetRoom(room RoomStateHandler) {
	worker.state.room.Store(room)
}

func (worker *Worker) closingCleanup() {
	worker.setRoomState()
	worker.roomHandler.BroadcastStatusList(worker)
}

func (worker *Worker) setRoomState() {
	room := worker.Room()
	if room == nil {
		return
	}

	uuid := worker.GetUUID()
	room.DeleteWorker(*uuid)

	if !room.IsEmpty() {
		return
	}

	room.SetPaused(true)
	if room.IsPlaylistEmpty() && room.IsPersistent() {
		worker.roomHandler.DeleteRoom(room)
	}
}

func (worker *Worker) handlePing(ping Ping, arrivalTime time.Time) {
	uuid, err := uuid.Parse(ping.Uuid)
	if err != nil {
		logger.Errorw("Unable to parse uuid", "error", err)
		return
	}

	worker.setRoundTripTime(uuid, arrivalTime)
}

func (worker *Worker) setRoundTripTime(uuid uuid.UUID, arrivalTime time.Time) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	timestamp, ok := worker.latency.timestamps[uuid]
	if !ok {
		logger.Warnw("Could not find ping for corresponding uuid", "uuid", uuid)
		return
	}

	newRoundTripTime := float64(arrivalTime.Sub(timestamp))
	worker.latency.roundTripTime = worker.latency.roundTripTime*latencyWeighingFactor + newRoundTripTime*(1-latencyWeighingFactor)

	delete(worker.latency.timestamps, uuid)
}

func (worker *Worker) handleStatus(status Status) {
	worker.SetUserStatus(status)
	worker.roomHandler.BroadcastStatusList(worker)
	room := worker.Room()
	room.BroadcastStartOnReady(worker)
}

func (worker *Worker) handleVideoStatus(videoStatus VideoStatus, arrivalTime time.Time) {
	room := worker.Room()
	roomState := room.RoomState()

	if videoStatus.Filename == nil || videoStatus.Position == nil {
		worker.handleNilStatus(videoStatus, roomState)
		return
	}

	if worker.isValidVideoStatus(videoStatus, roomState) {
		worker.SetVideoState(videoStatus, arrivalTime)
		room.HandleVideoStatus(worker)
	} else {
		worker.SendSeek(true)
	}
}

func (worker *Worker) handleNilStatus(videoStatus VideoStatus, roomState *RoomState) {
	if videoStatus.Filename != roomState.video || videoStatus.Position != roomState.position {
		worker.SendSeek(false)
	}
}

func (worker *Worker) isValidVideoStatus(videoStatus VideoStatus, roomState *RoomState) bool {
	// video status is not compatible with server if position is not in accordance with the last seek or video
	// is paused when it is not supposed to be
	if *videoStatus.Position < roomState.lastSeek || videoStatus.Paused != roomState.paused {
		return false
	}

	return true
}

func (worker *Worker) handleStart(start Start) {
	room := worker.Room()
	room.SetPaused(false)
	room.BroadcastStart(worker, false)
}

func (worker *Worker) handleSeek(seek Seek, arrivalTime time.Time) {
	room := worker.Room()
	room.SetPosition(seek.Position)
	room.SetLastSeek(seek.Position)
	worker.SetVideoState(VideoStatus{Filename: &seek.Filename, Position: &seek.Position, Paused: seek.Paused}, arrivalTime)
	room.BroadcastSeek(seek.Filename, seek.Position, worker, false)
}

func (worker *Worker) handleSelect(sel Select) {
	room := worker.Room()
	room.SetPlaylistState(sel.Filename, 0, true, 0)
	room.BroadcastSelect(sel.Filename, worker, false)
	room.BroadcastStartOnReady(worker)
}

func (worker *Worker) handlePlaylist(playlist Playlist) {
	room := worker.Room()
	room.HandlePlaylistUpdate(playlist.Playlist, worker)
	room.BroadcastPlaylist(playlist, worker, true)
}

func (worker *Worker) handlePause(pause Pause) {
	room := worker.Room()
	room.SetPaused(true)
	room.BroadcastPause(worker)
}

func (worker *Worker) handlePlaybackSpeed(speed PlaybackSpeed) {
	room := worker.Room()
	worker.setSpeed(speed.Speed)
	room.SetSpeed(speed.Speed)
	room.BroadcastPlaybackSpeed(speed.Speed, worker)
}

func (worker *Worker) SetVideoState(videoStatus VideoStatus, arrivalTime time.Time) {
	worker.videoStateMutex.Lock()
	defer worker.videoStateMutex.Unlock()

	worker.videoState.video = videoStatus.Filename
	worker.videoState.position = videoStatus.Position
	worker.videoState.timestamp = arrivalTime.Add(time.Duration(-worker.latency.roundTripTime/2) * time.Nanosecond)
	worker.videoState.paused = videoStatus.Paused
}

func (worker *Worker) setSpeed(speed float64) {
	worker.videoStateMutex.Lock()
	defer worker.videoStateMutex.Unlock()

	worker.videoState.speed = speed
}

func (worker *Worker) SendMessage(message []byte) {
	err := worker.webSocket.WriteMessage(message)
	if err != nil {
		logger.Errorw("Unable to send message", "error", err)
		worker.Close()
	}
}

func (worker *Worker) SendSeek(desync bool) {
	room := worker.Room()
	roomState := room.RoomState()

	// seeking nil videos is prohibited
	// may need to be changed to allow synchronization even if playlist is empty
	if roomState.video == nil || roomState.position == nil {
		return
	}

	// add half rtt if video is playing
	position := *roomState.position
	if !worker.videoState.paused {
		position += uint64(worker.latency.roundTripTime / float64(time.Millisecond) / 2)
	}

	userStatus := worker.GetUserStatus()
	seek := Seek{Filename: *roomState.video, Position: position, Speed: roomState.speed, Paused: roomState.paused, Desync: desync, Username: userStatus.Username}
	payload, err := MarshalMessage(seek)
	if err != nil {
		logger.Errorw("Capitalist failed to marshal seek", "error", err)
		return
	}

	worker.SendMessage(payload)
}

func (worker *Worker) SendServerMessage(message string, isError bool) {
	serverMessage := ServerMessage{Message: message, IsError: isError}
	data, err := MarshalMessage(serverMessage)
	if err != nil {
		logger.Errorw("Unable to parse server message", "error", err)
	}

	worker.SendMessage(data)
}

func (worker *Worker) SendPlaylist() {
	room := worker.Room()
	roomState := room.RoomState()
	userStatus := worker.GetUserStatus()

	playlist := Playlist{Playlist: roomState.playlist, Username: userStatus.Username}
	message, err := MarshalMessage(playlist)
	if err != nil {
		logger.Errorw("Unable to marshal playlist", "error", err)
		return
	}

	worker.SendMessage(message)
}

func (worker *Worker) EstimatePosition() uint64 {
	worker.videoStateMutex.RLock()
	defer worker.videoStateMutex.RUnlock()

	var estimatedPosition uint64
	if worker.videoState.paused {
		estimatedPosition = *worker.videoState.position
	} else {
		timeElapsed := uint64(float64(time.Since(worker.videoState.timestamp).Milliseconds()) * worker.videoState.speed)
		estimatedPosition = *worker.videoState.position + timeElapsed
	}

	return estimatedPosition
}

func (worker *Worker) DeleteWorkerFromRoom() {
	uuid := worker.GetUUID()
	worker.Room().DeleteWorker(*uuid)
}
