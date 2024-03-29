package communication

import (
	"fmt"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/sevenautumns/niketsu/server/src/logger"
)

// TODO handle clients with unstable latency
// TODO rate limiter
const (
	maxBufferedTasks      int     = 1000
	maxBufferedMessages   int     = 1000
	latencyWeighingFactor float64 = 0.85
)

var (
	pingTickInterval    Duration = Duration{time.Second}
	pingDeleteInterval  Duration = Duration{600 * time.Second}
	maxClientDifference Duration = Duration{time.Second}
	lastSeekEpsilon     Duration = Duration{500 * time.Millisecond}
)

type Task struct {
	payload     []byte
	arrivalTime time.Time
}

type ClientWorker interface {
	Start()
	Close()
	Shutdown()
	UUID() uuid.UUID
	SendMessage(payload []byte)
	EstimatePosition() *Duration
}

type workerLatency struct {
	roundTripTime Duration
	timestamps    map[uuid.UUID]time.Time
}

type workerVideoState struct {
	video      *string
	position   *Duration
	timestamp  time.Time
	paused     bool
	speed      float64
	fileLoaded bool
}

type workerState struct {
	uuid      uuid.UUID
	loggedIn  bool
	stopChan  chan int
	closeOnce sync.Once
	taskChan  chan Task
	writeChan chan []byte
}

type Worker struct {
	roomHandler     ServerStateHandler
	websocket       WebsocketReaderWriter
	room            RoomStateHandler
	state           workerState
	userStatus      Status
	videoState      workerVideoState
	videoStateMutex sync.RWMutex
	latency         workerLatency
	latencyMutex    sync.RWMutex
}

func NewWorker(roomHandler ServerStateHandler, websocket WebsocketReaderWriter, username string) ClientWorker {
	var worker Worker
	worker.roomHandler = roomHandler
	worker.websocket = websocket

	worker.state = workerState{
		uuid: uuid.New(), loggedIn: false, stopChan: make(chan int),
		taskChan:  make(chan Task, maxBufferedTasks),
		writeChan: make(chan []byte, maxBufferedMessages),
	}

	worker.room = nil
	worker.userStatus = Status{Username: username}
	worker.videoState = workerVideoState{paused: true, speed: 1.0}
	worker.latency = workerLatency{roundTripTime: Duration{0}, timestamps: make(map[uuid.UUID]time.Time)}

	return &worker
}

func (worker *Worker) Shutdown() {
	close(worker.state.stopChan)
}

func (worker *Worker) Close() {
	worker.state.closeOnce.Do(func() {
		close(worker.state.stopChan)

		worker.closingCleanup()
	})
}

func (worker *Worker) closingCleanup() {
	worker.setRoomStateAfterLeave()
	worker.roomHandler.BroadcastStatusList()
}

func (worker *Worker) setRoomStateAfterLeave() {
	if worker.room == nil {
		return
	}

	worker.room.DeleteWorker(worker.state.uuid)
	if worker.room.IsEmpty() {
		worker.room.SetPaused(true)
	}

	worker.deleteAndCloseEmptyRoom()
}

func (worker *Worker) deleteAndCloseEmptyRoom() {
	if worker.room.ShouldBeClosed() {
		err := worker.roomHandler.DeleteRoom(worker.room)
		if err != nil {
			logger.Warnw("Failed to delete room from handler")
		}

		err = worker.room.Close()
		if err != nil {
			logger.Warnw("Failed to close room")
		}
	}
}

func (worker *Worker) Start() {
	defer worker.websocket.Close()
	worker.init()

	var wg sync.WaitGroup
	wg.Add(4)
	go worker.handleReading(&wg)
	go worker.handleWriting(&wg)
	go worker.handleTasks(&wg)
	go worker.handlePings(&wg)
	wg.Wait()

	worker.closeTaskChan()
	worker.closeWriteChan()
}

func (worker *Worker) init() {
	worker.state.stopChan = make(chan int)
	worker.state.closeOnce = sync.Once{}
	worker.state.taskChan = make(chan Task, maxBufferedTasks)
	worker.state.writeChan = make(chan []byte, maxBufferedMessages)
}

func (worker *Worker) handlePings(wg *sync.WaitGroup) {
	var pingWG sync.WaitGroup
	pingWG.Add(2)

	pingTimer := worker.schedule(worker.sendPing, pingTickInterval.Duration, &pingWG)
	pingDeleteTimer := worker.schedule(worker.deletePings, pingDeleteInterval.Duration, &pingWG)
	defer pingTimer.Stop()
	defer pingDeleteTimer.Stop()

	pingWG.Wait()
	wg.Done()
}

func (worker *Worker) schedule(f func(), interval time.Duration, wg *sync.WaitGroup) *time.Ticker {
	ticker := time.NewTicker(interval)
	go func() {
		for {
			select {
			case <-worker.state.stopChan:
				wg.Done()
				return
			case <-ticker.C:
				f()
			}
		}
	}()

	return ticker
}

func (worker *Worker) sendPing() {
	workerUUID := uuid.New()
	payload, err := worker.getNewPing(workerUUID)
	if err != nil {
		logger.Errorw("Unable to parse ping message")
		return
	}

	worker.addPingEntry(workerUUID)
	worker.queueMessage(payload)
}

func (worker *Worker) getNewPing(workerUUID uuid.UUID) ([]byte, error) {
	ping := Ping{Uuid: workerUUID.String()}
	payload, err := MarshalMessage(ping)
	if err != nil {
		return nil, err
	}

	return payload, nil
}

func (worker *Worker) addPingEntry(workerUUID uuid.UUID) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	worker.latency.timestamps[workerUUID] = time.Now()
}

func (worker *Worker) deletePings() {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	for workerUUID, timestamp := range worker.latency.timestamps {
		elapsedTime := time.Since(timestamp)
		if elapsedTime > time.Minute {
			delete(worker.latency.timestamps, workerUUID)
		}
	}
}

func (worker *Worker) closeTaskChan() {
	close(worker.state.taskChan)
}

func (worker *Worker) closeWriteChan() {
	close(worker.state.writeChan)
}

func (worker *Worker) queueTask(task Task) {
	worker.state.taskChan <- task
}

func (worker *Worker) queueMessage(message []byte) {
	worker.state.writeChan <- message
}

func (worker *Worker) handleWriting(wg *sync.WaitGroup) {
	for {
		select {
		case <-worker.state.stopChan:
			wg.Done()
			return
		case message, ok := <-worker.state.writeChan:
			if !ok {
				worker.Close()
				wg.Done()
				return
			}

			worker.write(message)
		}
	}
}

func (worker *Worker) write(message []byte) {
	err := worker.websocket.WriteMessage(message)
	if err != nil {
		logger.Warnw("Unable to send message")
		worker.Close()
		return
	}
	logger.Debugw("Sent message to client", "message", string(message))
}

func (worker *Worker) handleReading(wg *sync.WaitGroup) {
	for {
		select {
		case <-worker.state.stopChan:
			wg.Done()
			return
		default:
			worker.read()
		}
	}
}

func (worker *Worker) read() {
	payload, err := worker.websocket.ReadMessage()
	logger.Debugw("Received message from client", "message", string(payload))

	arrivalTime := time.Now()
	if err != nil {
		logger.Warnw("Unable to read from client. Closing connection", "worker", worker.state.uuid)
		worker.Close()
		return
	}

	worker.queueTask(Task{payload: payload, arrivalTime: arrivalTime})
}

func (worker *Worker) handleTasks(wg *sync.WaitGroup) {
	for {
		select {
		case <-worker.state.stopChan:
			wg.Done()
			return
		case task, ok := <-worker.state.taskChan:
			if !ok {
				worker.Close()
				wg.Done()
				return
			}
			worker.work(task)
		}
	}
}

func (worker *Worker) work(task Task) {
	message, err := UnmarshalMessage(task.payload)
	if err != nil {
		logger.Errorw("Unable to unmarshal client message")
		return
	}

	if worker.isNotJoinAndNotLoggedIn(message) {
		return
	}

	worker.handleMessageTypes(message, task.arrivalTime)
}

func (worker *Worker) isNotJoinAndNotLoggedIn(message Message) bool {
	return !worker.state.loggedIn && message.Type() != JoinType
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
		worker.handleJoin(message)
	case PlaybackSpeedType:
		message := message.(*PlaybackSpeed)
		worker.handlePlaybackSpeed(*message)
	default:
		serverMessage := fmt.Sprintf("Requested command %s not supported:", message.Type())
		worker.sendServerMessage(serverMessage, true)
	}
}

func (worker *Worker) sendServerMessage(message string, isError bool) {
	serverMessage := ServerMessage{Message: message, IsError: isError}
	payload, err := MarshalMessage(serverMessage)
	if err != nil {
		logger.Errorw("Unable to parse server message")
	}

	worker.queueMessage(payload)
}

func (worker *Worker) UUID() uuid.UUID {
	return worker.state.uuid
}

func (worker *Worker) SetUserStatus(status Status) {
	worker.userStatus = status
}

func (worker *Worker) VideoState() workerVideoState {
	worker.videoStateMutex.RLock()
	defer worker.videoStateMutex.RUnlock()

	return worker.videoState
}

func (worker *Worker) handlePing(ping Ping, arrivalTime time.Time) {
	workerUUID, err := uuid.Parse(ping.Uuid)
	if err != nil {
		logger.Errorw("Unable to parse uuid")
		return
	}

	worker.setRoundTripTime(workerUUID, arrivalTime)
	worker.detelePing(workerUUID)
}

func (worker *Worker) setRoundTripTime(workerUUID uuid.UUID, arrivalTime time.Time) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	timestamp, ok := worker.latency.timestamps[workerUUID]
	if !ok {
		logger.Warnw("Could not find ping for corresponding uuid", "uuid", workerUUID)
		return
	}

	worker.latency.roundTripTime = worker.calculateNewRoundTripTime(arrivalTime, timestamp)
}

func (worker *Worker) calculateNewRoundTripTime(arrivalTime time.Time, timestamp time.Time) Duration {
	newRoundTripTime := TimeSub(arrivalTime, timestamp)
	return worker.latency.roundTripTime.MultFloat64(latencyWeighingFactor).Add(newRoundTripTime.MultFloat64(1 - latencyWeighingFactor))
}

func (worker *Worker) detelePing(workerUUID uuid.UUID) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	delete(worker.latency.timestamps, workerUUID)
}

func (worker *Worker) handleStatus(status Status) {
	if worker.isStatusNew(status) {
		worker.handleDuplicateUsername(status)
		worker.room.SetWorkerStatus(worker.state.uuid, worker.userStatus)
		worker.roomHandler.BroadcastStatusList()
		worker.broadcastStartOnReady()
	}
}

func (worker *Worker) handleDuplicateUsername(status Status) {
	if worker.userStatus.Username != status.Username {
		newUsername := worker.roomHandler.RenameUserIfUnavailable(status.Username)
		if newUsername != status.Username {
			status.Username = newUsername
			worker.sendUserStatus(status)
		}
	}
	worker.SetUserStatus(status)
}

func (worker *Worker) isStatusNew(status Status) bool {
	return status.Ready != worker.userStatus.Ready || status.Username != worker.userStatus.Username
}

func (worker *Worker) handleVideoStatus(videoStatus VideoStatus, arrivalTime time.Time) {
	logger.Debugw("Received video status", "videostatus", videoStatus)
	fileLoaded := worker.VideoState().fileLoaded
	worker.setVideoState(videoStatus, arrivalTime)

	if !fileLoaded && videoStatus.FileLoaded {
		worker.sendSeek(false)
		return
	}

	roomState := worker.room.RoomState()
	if worker.isVideoStateDifferent(videoStatus, roomState) {
		worker.sendSelect(roomState.video, *roomState.position)
		return
	}

	if videoStatus.Speed != roomState.speed {
		worker.sendSpeed(roomState.speed)
		return
	}

	if videoStatus.Paused != roomState.paused {
		worker.sendPausePlay(videoStatus.Paused)
		return
	}

	worker.handleTimeDifference(videoStatus)
	worker.room.HandleCache(videoStatus.Cache, worker.state.uuid, worker.userStatus.Username)
}

func (worker *Worker) isVideoStateDifferent(videoStatus VideoStatus, roomState RoomState) bool {
	return (videoStatus.Filename == nil && roomState.video != nil) ||
		(videoStatus.Filename != nil && roomState.video != nil &&
			*videoStatus.Filename != *roomState.video)
}

func (worker *Worker) sendSelect(filename *string, position Duration) {
	sel := Select{Filename: filename, Position: position, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(sel)
	if err != nil {
		logger.Warnw("Failed to marshal select message")
		return
	}

	worker.queueMessage(payload)
}

func (worker *Worker) sendSpeed(speed float64) {
	msg := PlaybackSpeed{Speed: speed, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(msg)
	if err != nil {
		logger.Warnw("Failed to marshal speed message")
		return
	}

	worker.queueMessage(payload)
}

func (worker *Worker) sendPausePlay(paused bool) {
	var message Message
	if paused {
		message = Start{Username: worker.userStatus.Username}
	} else {
		message = Pause{Username: worker.userStatus.Username}
	}

	payload, err := MarshalMessage(message)
	if err != nil {
		logger.Warnw("Failed to marshal pause/start message")
		return
	}

	worker.queueMessage(payload)
}

func (worker *Worker) handleStart(start Start) {
	worker.setPause(false)
	worker.room.SetPaused(false)
	worker.broadcastStart()
}

func (worker *Worker) setPause(pause bool) {
	worker.videoStateMutex.Lock()
	defer worker.videoStateMutex.Unlock()

	worker.videoState.paused = pause
}

func (worker *Worker) handleSeek(seek Seek, arrivalTime time.Time) {
	worker.room.SetPlaylistState(&seek.Filename, seek.Position, nil, &seek.Position, nil)
	worker.setVideoState(VideoStatus{Filename: &seek.Filename, Position: &seek.Position}, arrivalTime)
	worker.broadcastSeek(seek.Filename, seek.Position, false)
}

func (worker *Worker) handleSelect(sel Select) {
	paused := true
	worker.room.SetPlaylistState(sel.Filename, sel.Position, &paused, &sel.Position, nil)
	worker.setVideoState(VideoStatus{Filename: sel.Filename, Position: &sel.Position,
		Paused: true, Speed: -1}, time.Now())
	worker.broadcastSelect(sel.Filename, sel.Position, false)
	worker.broadcastStartOnReady()
}

func (worker *Worker) handlePlaylist(playlist Playlist) {
	worker.handlePlaylistUpdate(playlist.Playlist)
	worker.broadcastPlaylist(playlist)
}

func (worker *Worker) handlePause(pause Pause) {
	worker.setPause(true)
	worker.room.SetPaused(true)
	worker.broadcastPause()
}

func (worker *Worker) handlePlaybackSpeed(speed PlaybackSpeed) {
	worker.setSpeed(speed.Speed)
	worker.room.SetSpeed(speed.Speed)
	worker.broadcastPlaybackSpeed(speed.Speed)
}

func (worker *Worker) handleJoin(join Join) {
	logger.Debugw("Received login attempt", "message", join)
	if !worker.roomHandler.IsPasswordCorrect(join.Password) {
		logger.Warnw("Room access failed due to incorrect password")
		worker.sendServerMessage("Password is incorrect. Please try again", true)
		return
	}

	// in case of a room change, try to delete the previous room
	if worker.state.loggedIn {
		worker.setRoomStateAfterLeave()
	}

	// rename user if duplicated
	status := Status{Username: join.Username}
	worker.handleDuplicateUsername(status)

	err := worker.handleRoomJoin(join)
	if err != nil {
		worker.sendServerMessage("Failed to access room. Please try again", true)
		return
	}

	worker.state.loggedIn = true
}

func (worker *Worker) sendUserStatus(status Status) {
	payload, err := MarshalMessage(status)
	if err != nil {
		logger.Warnw("Failed to marshal username message")
		return
	}

	worker.queueMessage(payload)

}

func (worker *Worker) handleRoomJoin(join Join) error {
	err := worker.updateRoomChangeState(join.Room)
	if err != nil {
		return err
	}

	worker.sendRoomChangeUpdates()
	return nil
}

func (worker *Worker) updateRoomChangeState(roomName string) error {
	room, err := worker.roomHandler.CreateOrFindRoom(roomName)
	if err != nil {
		return err
	}

	err = worker.roomHandler.AppendRoom(room)
	if err != nil {
		return err
	}

	room.AppendWorker(worker)
	roomState := room.RoomState()
	worker.setVideoState(VideoStatus{Filename: roomState.video, Position: roomState.position,
		Paused: roomState.paused, Speed: roomState.speed}, time.Now())
	worker.room = room
	worker.room.SetWorkerStatus(worker.state.uuid, worker.userStatus)

	return nil
}

func (worker *Worker) sendRoomChangeUpdates() {
	worker.sendPlaylist()
	roomState := worker.room.RoomState()
	if roomState.video != nil && roomState.position != nil {
		worker.sendSelect(roomState.video, *roomState.position)
	}
	worker.roomHandler.BroadcastStatusList()
}

func (worker *Worker) setSpeed(speed float64) {
	worker.videoStateMutex.Lock()
	defer worker.videoStateMutex.Unlock()

	worker.videoState.speed = speed
}

func (worker *Worker) roundTripTime() Duration {
	worker.latencyMutex.RLock()
	defer worker.latencyMutex.RUnlock()

	return worker.latency.roundTripTime
}

func (worker *Worker) setVideoState(videoStatus VideoStatus, arrivalTime time.Time) {
	worker.videoStateMutex.Lock()
	defer worker.videoStateMutex.Unlock()

	worker.videoState.video = videoStatus.Filename
	if videoStatus.Position != nil {
		worker.videoState.position = videoStatus.Position
	}

	worker.videoState.timestamp = TimeAdd(arrivalTime, worker.roundTripTime().Div(2).Negate())
	worker.videoState.paused = videoStatus.Paused
	if videoStatus.Speed > 0 {
		worker.videoState.speed = videoStatus.Speed
	}

	worker.videoState.fileLoaded = videoStatus.FileLoaded
}

func (worker *Worker) sendSeek(desync bool) {
	roomState := worker.room.RoomState()
	// seeking nil videos is prohibited
	// may need to be changed to allow synchronization even if playlist is empty
	if roomState.video == nil || roomState.position == nil {
		return
	}

	// add half rtt if video is playing
	position := *roomState.position
	if !worker.paused() {
		position = position.Add(worker.roundTripTime().Div(2))
	}

	seek := Seek{Filename: *roomState.video, Position: position, Desync: desync, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(seek)
	if err != nil {
		logger.Errorw("Failed to marshal seek")
		return
	}

	worker.room.SetLastSeek(position)
	worker.queueMessage(payload)
}

func (worker *Worker) paused() bool {
	worker.videoStateMutex.RLock()
	worker.videoStateMutex.RUnlock()

	return worker.videoState.paused
}

func (worker *Worker) SendMessage(payload []byte) {
	worker.queueMessage(payload)
}

func (worker *Worker) sendPlaylist() {
	roomState := worker.room.RoomState()

	playlist := Playlist{Playlist: roomState.playlist, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(playlist)
	if err != nil {
		logger.Errorw("Unable to marshal playlist")
		return
	}

	worker.queueMessage(payload)
}

func (worker *Worker) EstimatePosition() *Duration {
	videoState := worker.VideoState()

	if videoState.position == nil || !videoState.fileLoaded {
		return nil
	}

	var estimatedPosition Duration
	if videoState.paused {
		estimatedPosition = *videoState.position
	} else {
		timeElapsed := TimeSince(videoState.timestamp).MultFloat64(videoState.speed)
		estimatedPosition = videoState.position.Add(timeElapsed)
	}

	return &estimatedPosition
}

func (worker *Worker) broadcastStart() {
	start := Start{Username: worker.userStatus.Username}
	payload, err := MarshalMessage(start)
	if err != nil {
		logger.Errorw("Unable to marshal start message")
		return
	}

	worker.room.BroadcastExcept(payload, worker.state.uuid)
}

func (worker *Worker) broadcastSeek(filename string, position Duration, desync bool) {
	seek := Seek{Filename: filename, Position: position, Desync: desync, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(seek)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast seek")
		return
	}

	workerUUID := worker.UUID()
	worker.room.BroadcastExcept(payload, workerUUID)
}

func (worker *Worker) broadcastSelect(filename *string, position Duration, all bool) {
	sel := Select{Filename: filename, Position: position, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(sel)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast select")
		return
	}

	worker.room.BroadcastAll(payload)
}

func (worker *Worker) broadcastUserMessage(message string) {
	userMessage := UserMessage{Message: message, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(userMessage)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast user message")
		return
	}

	workerUUID := worker.UUID()
	worker.room.BroadcastExcept(payload, workerUUID)
}

func (worker *Worker) broadcastPlaylist(playlist Playlist) {
	pl := Playlist{Playlist: playlist.Playlist, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(pl)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playlist")
		return
	}

	worker.room.BroadcastAll(payload)
}

func (worker *Worker) broadcastPause() {
	pause := Pause{Username: worker.userStatus.Username}
	payload, err := MarshalMessage(pause)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast pause")
		return
	}

	workerUUID := worker.UUID()
	worker.room.BroadcastExcept(payload, workerUUID)
}

func (worker *Worker) broadcastStartOnReady() {
	roomState := worker.room.RoomState()
	if roomState.video == nil {
		return
	}

	if !roomState.paused {
		return
	}

	if worker.room.Ready() {
		start := Start{Username: worker.userStatus.Username}
		payload, err := MarshalMessage(start)
		if err != nil {
			logger.Errorw("Unable to marshal start message")
			return
		}

		worker.room.BroadcastAll(payload)
		worker.room.SetPaused(false)
	}
}

func (worker *Worker) broadcastPlaybackSpeed(speed float64) {
	playbackSpeed := PlaybackSpeed{Speed: speed, Username: worker.userStatus.Username}
	payload, err := MarshalMessage(playbackSpeed)
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playbackspeed")
		return
	}

	workerUUID := worker.UUID()
	worker.room.BroadcastExcept(payload, workerUUID)
}

func (worker *Worker) handlePlaylistUpdate(playlist []string) {
	state := worker.room.RoomState()

	if worker.isPlaylistUpdateRequired(state.video, state.playlist, playlist) {
		nextVideo := worker.findNext(playlist, state)
		worker.setNextVideo(nextVideo, *state.video, worker.room)
	}

	worker.room.SetPlaylist(playlist)
}

func (worker *Worker) isPlaylistUpdateRequired(video *string, oldPlaylist []string, newPlaylist []string) bool {
	return video != nil && len(newPlaylist) != 0 && len(newPlaylist) < len(oldPlaylist)
}

func (worker *Worker) findNext(newPlaylist []string, state RoomState) string {
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
		paused := true
		room.SetPlaylistState(&nextVideo, Duration{0}, &paused, &Duration{0}, nil)
		worker.broadcastSelect(&nextVideo, Duration{0}, true)
	}
}

// Evaluates the video states of all clients and broadcasts seek if difference between
// fastest and slowest clients is too large. Can not seek before the last seek's position.
func (worker *Worker) handleTimeDifference(videoStatus VideoStatus) {
	minPosition := worker.room.SlowestEstimatedClientPosition()
	if minPosition == nil {
		return
	}

	if worker.shouldSeek(minPosition, videoStatus.Position, videoStatus.Speed) {
		worker.room.SetPosition(*minPosition)
		worker.sendSeek(true)
	} else {
		worker.room.SetPosition(*videoStatus.Position)
	}
}

func (worker *Worker) shouldSeek(minPosition *Duration, workerPosition *Duration, speed float64) bool {
	lastSeek := worker.room.RoomState().lastSeek.Sub(lastSeekEpsilon)
	if workerPosition == nil || workerPosition.Smaller(lastSeek) {
		return true
	}

	if minPosition == nil || minPosition.Smaller(lastSeek) {
		return false
	}

	clientDifference := workerPosition.Sub(*minPosition)
	adjustedMaxClientDifference := maxClientDifference.MultFloat64(speed)

	return workerPosition.Greater(*minPosition) && clientDifference.Greater(adjustedMaxClientDifference)
}
