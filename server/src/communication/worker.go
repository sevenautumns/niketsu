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
	UUID() *uuid.UUID
	SetUserStatus(status Status)
	UserStatus() *Status
	SetVideoState(videoStatus VideoStatus, arrivalTime time.Time)
	Login()
	LoggedIn() bool
	SendMessage(payload []byte)
	SendServerMessage(message string, isError bool)
	SendSeek(desync bool)
	SendPlaylist()
	EstimatePosition() *uint64
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
	closeOnce   *sync.Once
}

type Worker struct {
	roomHandler     ServerStateHandler
	websocket       WebsocketReaderWriter
	state           workerState
	userStatus      *atomic.Pointer[Status]
	videoState      *videoState
	videoStateMutex *sync.RWMutex
	latency         *latency
	latencyMutex    *sync.RWMutex
}

func NewWorker(roomHandler ServerStateHandler, webSocket WebsocketReaderWriter, userName string) ClientWorker {
	var worker Worker
	worker.roomHandler = roomHandler
	worker.websocket = webSocket

	atomicUUID := atomic.Pointer[uuid.UUID]{}
	newUUID := uuid.New()
	atomicUUID.Store(&newUUID)
	atomicBool := atomic.Bool{}
	atomicBool.Store(false)
	worker.state = workerState{uuid: &atomicUUID, room: &atomic.Value{}, loggedIn: &atomicBool, stopRequest: make(chan int), closeOnce: &sync.Once{}}

	atomicStatus := atomic.Pointer[Status]{}
	atomicStatus.Store(&Status{Ready: false, Username: userName})
	worker.userStatus = &atomicStatus

	worker.videoState = &videoState{paused: true, speed: 1.0}
	worker.videoStateMutex = &sync.RWMutex{}

	worker.latency = &latency{roundTripTime: 0, timestamps: make(map[uuid.UUID]time.Time)}
	worker.latencyMutex = &sync.RWMutex{}

	return &worker
}

func (worker *Worker) UUID() *uuid.UUID {
	return worker.state.uuid.Load()
}

func (worker *Worker) SetUserStatus(status Status) {
	worker.userStatus.Store(&status)
}

func (worker *Worker) UserStatus() *Status {
	return worker.userStatus.Load()
}

func (worker *Worker) VideoState() *videoState {
	worker.videoStateMutex.RLock()
	defer worker.videoStateMutex.RUnlock()

	return worker.videoState
}

func (worker *Worker) Login() {
	worker.state.loggedIn.Store(true)
}

func (worker *Worker) LoggedIn() bool {
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
	defer worker.websocket.Close()

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

	err = worker.websocket.WriteMessage(payload)
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
	data, err := worker.websocket.ReadMessage()
	arrivalTime := time.Now()

	if err != nil {
		logger.Infow("Unable to read from client. Closing connection", "error", err, "worker", worker.state.uuid)
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
	if !worker.LoggedIn() && message.Type() != JoinType {
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
		worker.broadcastUserMessage(message.Message)
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

	uuid := worker.UUID()
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
	worker.broadcastStartOnReady()
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
		worker.handleTimeDifference()
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
	worker.broadcastStart()
}

func (worker *Worker) handleSeek(seek Seek, arrivalTime time.Time) {
	room := worker.Room()
	room.SetPlaylistState(&seek.Filename, seek.Position, seek.Paused, seek.Position)
	worker.SetVideoState(VideoStatus{Filename: &seek.Filename, Position: &seek.Position, Paused: seek.Paused}, arrivalTime)
	worker.broadcastSeek(seek.Filename, seek.Position, false)
}

func (worker *Worker) handleSelect(sel Select) {
	room := worker.Room()
	room.SetPlaylistState(sel.Filename, 0, true, 0)
	worker.broadcastSelect(sel.Filename, false)
	worker.broadcastStartOnReady()
}

func (worker *Worker) handlePlaylist(playlist Playlist) {
	worker.HandlePlaylistUpdate(playlist.Playlist)
	worker.broadcastPlaylist(playlist, true)
}

func (worker *Worker) handlePause(pause Pause) {
	room := worker.Room()
	room.SetPaused(true)
	worker.broadcastPause()
}

func (worker *Worker) handlePlaybackSpeed(speed PlaybackSpeed) {
	room := worker.Room()
	worker.setSpeed(speed.Speed)
	room.SetSpeed(speed.Speed)
	worker.broadcastPlaybackSpeed(speed.Speed)
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
	err := worker.websocket.WriteMessage(message)
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

	userStatus := worker.UserStatus()
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
	userStatus := worker.UserStatus()

	playlist := Playlist{Playlist: roomState.playlist, Username: userStatus.Username}
	message, err := MarshalMessage(playlist)
	if err != nil {
		logger.Errorw("Unable to marshal playlist", "error", err)
		return
	}

	worker.SendMessage(message)
}

func (worker *Worker) EstimatePosition() *uint64 {
	worker.videoStateMutex.RLock()
	defer worker.videoStateMutex.RUnlock()

	if worker.videoState.position == nil {
		return nil
	}

	var estimatedPosition uint64
	if worker.videoState.paused {
		estimatedPosition = *worker.videoState.position
	} else {
		timeElapsed := uint64(float64(time.Since(worker.videoState.timestamp).Milliseconds()) * worker.videoState.speed)
		estimatedPosition = *worker.videoState.position + timeElapsed
	}

	return &estimatedPosition
}

func (worker *Worker) DeleteWorkerFromRoom() {
	uuid := worker.UUID()
	worker.Room().DeleteWorker(*uuid)
}

func (worker *Worker) broadcastStart() {
	userStatus := worker.UserStatus()
	start := Start{Username: userStatus.Username}
	payload, err := MarshalMessage(start)
	if err != nil {
		logger.Errorw("Unable to marshal start message", "error", err)
		return
	}

	room := worker.Room()
	uuid := worker.UUID()
	room.BroadcastExcept(payload, *uuid)
}

func (worker *Worker) broadcastSeek(filename string, position uint64, desync bool) {
	userStatus := worker.UserStatus()
	room := worker.Room()
	state := room.RoomState()

	seek := Seek{Filename: filename, Position: position, Speed: state.speed, Paused: state.paused, Desync: desync, Username: userStatus.Username}
	payload, err := MarshalMessage(seek)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast seek", "error", err)
		return
	}

	uuid := worker.UUID()
	room.BroadcastExcept(payload, *uuid)
}

func (worker *Worker) broadcastSelect(filename *string, all bool) {
	userStatus := worker.UserStatus()
	sel := Select{Filename: filename, Username: userStatus.Username}
	payload, err := MarshalMessage(sel)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast select", "error", err)
		return
	}
	room := worker.Room()

	if all {
		room.BroadcastAll(payload)
	} else {
		uuid := worker.UUID()
		room.BroadcastExcept(payload, *uuid)
	}
}

func (worker *Worker) broadcastUserMessage(message string) {
	userStatus := worker.UserStatus()
	userMessage := UserMessage{Message: message, Username: userStatus.Username}
	payload, err := MarshalMessage(userMessage)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast user message", "error", err)
		return
	}

	room := worker.Room()
	uuid := worker.UUID()
	room.BroadcastExcept(payload, *uuid)
}

func (worker *Worker) broadcastPlaylist(playlist Playlist, all bool) {
	userStatus := worker.UserStatus()
	pl := Playlist{Playlist: playlist.Playlist, Username: userStatus.Username}
	payload, err := MarshalMessage(pl)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playlist", "error", err)
		return
	}

	room := worker.Room()
	if all {
		room.BroadcastAll(payload)
	} else {
		uuid := worker.UUID()
		room.BroadcastExcept(payload, *uuid)
	}
}

func (worker *Worker) broadcastPause() {
	userStatus := worker.UserStatus()
	pause := Pause{Username: userStatus.Username}
	payload, err := MarshalMessage(pause)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast pause", "error", err)
		return
	}

	room := worker.Room()
	uuid := worker.UUID()
	room.BroadcastExcept(payload, *uuid)
}

// set paused to false since video will start
func (worker *Worker) broadcastStartOnReady() {
	// cannot start nil video
	room := worker.Room()
	video := room.RoomState().video
	if video == nil {
		return
	}

	if room.AllUsersReady() {
		userStatus := worker.UserStatus()
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

func (worker *Worker) broadcastPlaybackSpeed(speed float64) {
	userStatus := worker.UserStatus()
	playbackSpeed := PlaybackSpeed{Speed: speed, Username: userStatus.Username}
	payload, err := MarshalMessage(playbackSpeed)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playbackspeed", "error", err)
		return
	}

	room := worker.Room()
	uuid := worker.UUID()
	room.BroadcastExcept(payload, *uuid)
}

func (worker *Worker) HandlePlaylistUpdate(playlist []string) {
	room := worker.Room()
	state := room.RoomState()

	if worker.isPlaylistUpdateRequired(state.video, state.playlist, playlist) {
		nextVideo := worker.findNext(playlist, state)
		worker.setNextVideo(nextVideo, *state.video, room)
	}
}

func (worker *Worker) isPlaylistUpdateRequired(video *string, oldPlaylist []string, newPlaylist []string) bool {
	return video != nil && len(newPlaylist) != 0 && len(newPlaylist) < len(oldPlaylist)
}

func (worker *Worker) findNext(newPlaylist []string, state *RoomState) string {
	newPlaylistPosition := 0

	for _, video := range state.playlist {
		if video == *state.video {
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

func (worker *Worker) setNextVideo(nextVideo string, oldVideo string, room RoomStateHandler) {
	if nextVideo != oldVideo {
		room.SetPlaylistState(&nextVideo, 0, true, 0)
		worker.broadcastSelect(&nextVideo, true)
	}
}

// Evaluates the video states of all clients and broadcasts seek if difference between
// fastest and slowest clients is too large. Can not seek before the last seek's position.
func (worker *Worker) handleTimeDifference() {
	room := worker.Room()
	maxPosition := room.FastestClientPosition()
	state := worker.VideoState()
	room.SetPosition(*state.position)

	if worker.isClientDifferenceTooLarge(maxPosition, state.position, state.speed) {
		worker.SendSeek(true)
	}
}

func (worker *Worker) isClientDifferenceTooLarge(maxPosition uint64, workerPosition *uint64, speed float64) bool {
	return workerPosition == nil || maxPosition-*workerPosition > uint64(float64(maxClientDifferenceMillisecodns)*speed)
}
