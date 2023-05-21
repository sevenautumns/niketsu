package niketsu_server

import (
	"fmt"
	"net"
	"sync"
	"time"

	"github.com/gobwas/ws"
	"github.com/gobwas/ws/wsutil"
	"github.com/google/uuid"
)

// TODO check if order of lock acquiring is consistent
// TODO handle clients with unstable latency
// TODO fix start on new video, update status of client when select
// TODO keep status list up to date to reduce acquisition
// TODO locked getter for userstatus
const (
	LATENCY_WEIGHTING_FACTOR           float64       = 0.85
	PING_TICK_INTERVAL                 time.Duration = time.Second
	PING_DELETE_INTERVAL               time.Duration = 600 * time.Second
	MAX_CLIENT_DIFFERENCE_MILLISECONDS uint64        = 1e3 // one second
	//UNSTABLE_LATENCY_THRESHOLD  float64       = 2e3
)

type Latency struct {
	roundTripTime float64
	timestamps    map[uuid.UUID]time.Time
}

type Video struct {
	filename  *string
	position  *uint64
	timestamp time.Time
	paused    bool
	speed     float64
}

type WorkerSettings struct {
	uuid           uuid.UUID
	conn           net.Conn
	room           *Room
	loggedIn       bool
	serviceChannel chan int
	closeOnce      sync.Once
}

type Worker struct {
	overseer         *Overseer
	settings         *WorkerSettings
	settingsMutex    *sync.RWMutex
	userStatus       *Status
	userStatusMutex  *sync.RWMutex
	videoStatus      *Video
	videoStatusMutex *sync.RWMutex
	latency          *Latency
	latencyMutex     *sync.RWMutex
}

func NewWorker(overseer *Overseer, conn net.Conn, userName string, filename *string, position *uint64) Worker {
	var worker Worker
	worker.overseer = overseer
	worker.settings = &WorkerSettings{uuid: uuid.New(), conn: conn, room: nil, loggedIn: false, serviceChannel: make(chan int)}
	worker.settingsMutex = &sync.RWMutex{}
	worker.userStatus = &Status{Ready: false, Username: userName}
	worker.userStatusMutex = &sync.RWMutex{}
	worker.videoStatus = &Video{filename: filename, position: position, paused: true, speed: 1.0}
	worker.videoStatusMutex = &sync.RWMutex{}
	worker.latency = &Latency{roundTripTime: 0, timestamps: make(map[uuid.UUID]time.Time)}
	worker.latencyMutex = &sync.RWMutex{}

	return worker
}

func (worker *Worker) Start() {
	defer worker.settings.conn.Close()

	go worker.handlerService()
	go worker.pingService()
	<-worker.settings.serviceChannel
}

func (worker *Worker) pingService() {
	pingTimer := worker.schedule(worker.sendPing, PING_TICK_INTERVAL)
	pingDeleteTimer := worker.schedule(worker.deletePings, PING_DELETE_INTERVAL)
	defer pingTimer.Stop()
	defer pingDeleteTimer.Stop()

	<-worker.settings.serviceChannel
}

func (worker *Worker) schedule(f func(), interval time.Duration) *time.Ticker {
	ticker := time.NewTicker(interval)
	go func() {
		for {
			select {
			case <-ticker.C:
				f()
			case <-worker.settings.serviceChannel:
				return
			}
		}
	}()
	return ticker
}

func (worker *Worker) sendPing() {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	uuid := uuid.New()
	payload, err := worker.generateNewPing(uuid)
	if err != nil {
		logger.Errorw("Unable to parse ping message", "error", err)
		return
	}
	worker.addPingEntry(uuid)

	err = wsutil.WriteServerMessage(worker.settings.conn, ws.OpText, payload)
	if err != nil {
		logger.Errorw("Unable to send ping message", "error", err)
		worker.close()
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

// TODO low level api implementation
func (worker *Worker) handlerService() {
	for {
		select {
		case <-worker.settings.serviceChannel:
			return
		default:
			worker.handleData()
		}
	}
}

func (worker *Worker) handleData() {
	data, op, err := wsutil.ReadClientData(worker.settings.conn)
	arrivalTime := time.Now()

	if err != nil {
		logger.Errorw("Unable to read from client. Closing connection", "error", err, "worker", worker.settings.uuid)
		worker.close()
		return
	}

	//TODO handle different op code
	if op == ws.OpClose {
		logger.Infow("Client closed connection")
		worker.close()
		return
	}

	go worker.handleMessage(data, arrivalTime)
}

func (worker *Worker) handleMessage(data []byte, arrivalTime time.Time) {
	defer logger.Debugw("Time passed to handle message", "time", time.Now().Sub(arrivalTime))

	message, err := UnmarshalMessage(data)
	if err != nil {
		logger.Errorw("Unable to unmarshal client message", "error", err)
		return
	}
	logger.Debugw("Received message from client", "name", worker.userStatus.Username, "type", message.Type(), "message", message)

	if worker.ignoreNotLoggedIn(message) {
		return
	}

	worker.handleMessageTypes(message, arrivalTime)
}

func (worker *Worker) ignoreNotLoggedIn(message Message) bool {
	worker.settingsMutex.RLock()
	defer worker.settingsMutex.RUnlock()

	if !worker.settings.loggedIn && message.Type() != JoinType {
		return true
	}

	return false
}

func (worker *Worker) handleMessageTypes(message Message, arrivalTime time.Time) {
	switch message.Type() {
	case PingType:
		message := message.(*Ping)
		worker.handlePing(message, arrivalTime)
	case StatusType:
		message := message.(*Status)
		worker.handleStatus(message)
	case VideoStatusType:
		message := message.(*VideoStatus)
		worker.handleVideoStatus(message, arrivalTime)
	case StartType:
		message := message.(*Start)
		worker.handleStart(message)
	case SeekType:
		message := message.(*Seek)
		worker.handleSeek(message, arrivalTime)
	case SelectType:
		message := message.(*Select)
		worker.handleSelect(message)
	case UserMessageType:
		message := message.(*UserMessage)
		worker.settings.room.broadcastUserMessage(message.Message, worker)
	case PlaylistType:
		message := message.(*Playlist)
		worker.handlePlaylist(message)
	case PauseType:
		message := message.(*Pause)
		worker.handlePause(message)
	case JoinType:
		message := message.(*Join)
		worker.overseer.handleJoin(message, worker)
	case PlaybackSpeedType:
		message := message.(*PlaybackSpeed)
		worker.handlePlaybackSpeed(message)
	default:
		serverMessage := fmt.Sprintf("Requested command %s not supported", message.Type())
		worker.sendServerMessage(serverMessage, true)
	}
}

func (worker *Worker) close() {
	worker.settings.closeOnce.Do(func() {
		close(worker.settings.serviceChannel)

		if worker.settings.room != nil {
			worker.closingCleanup()
		}
	})
}

func (worker *Worker) closingCleanup() {
	worker.settings.room.deleteWorker(worker)
	worker.settings.room.checkRoomState(worker)
	worker.overseer.broadcastStatusList(worker)
}

func (worker *Worker) handlePing(ping *Ping, arrivalTime time.Time) {
	uuid, err := uuid.Parse(ping.Uuid)
	if err != nil {
		logger.Errorw("Unable to parse uuid", "error", err)
		return
	}

	worker.updateRoundTripTime(uuid, arrivalTime)
}

func (worker *Worker) updateRoundTripTime(uuid uuid.UUID, arrivalTime time.Time) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	timestamp, ok := worker.latency.timestamps[uuid]
	if !ok {
		logger.Warnw("Could not find ping for corresponding uuid", "uuid", uuid)
		return
	}

	newRoundTripTime := float64(arrivalTime.Sub(timestamp))
	worker.latency.roundTripTime = worker.latency.roundTripTime*LATENCY_WEIGHTING_FACTOR + newRoundTripTime*(1-LATENCY_WEIGHTING_FACTOR)

	delete(worker.latency.timestamps, uuid)
}

func (worker *Worker) handleStatus(status *Status) {
	worker.updateUserStatus(status)
	worker.overseer.broadcastStatusList(worker)
	worker.settings.room.broadcastStartOnReady(worker)
}

func (worker *Worker) updateUserStatus(status *Status) {
	worker.userStatusMutex.Lock()
	defer worker.userStatusMutex.Unlock()

	worker.userStatus = status
}

func (worker *Worker) getUserStatus() {

}

func (worker *Worker) handleVideoStatus(videoStatus *VideoStatus, arrivalTime time.Time) {
	if videoStatus.Filename == nil || videoStatus.Position == nil {
		worker.settings.room.handleNilStatus(videoStatus, worker)
		return
	}

	if worker.settings.room.isValidVideoStatus(videoStatus, worker) {
		worker.updateVideoStatus(videoStatus, arrivalTime)
		worker.settings.room.handleVideoStatus(worker)
	} else {
		worker.settings.room.sendSeekWithLock(worker, true)
	}
}

func (worker *Worker) handleStart(start *Start) {
	worker.settings.room.setPaused(false)
	worker.settings.room.broadcastStart(worker, false)
}

func (worker *Worker) handleSeek(seek *Seek, arrivalTime time.Time) {
	worker.settings.room.changePosition(seek.Position)
	worker.settings.room.updateLastSeek(seek.Position)
	worker.updateVideoStatus(&VideoStatus{Filename: &seek.Filename, Position: &seek.Position, Paused: seek.Paused}, arrivalTime)
	worker.settings.room.broadcastSeek(seek.Filename, seek.Position, worker, false)
}

func (worker *Worker) handleSelect(sel *Select) {
	worker.settings.room.changePlaylistStateWithLock(sel.Filename, 0, true, 0)
	worker.settings.room.broadcastSelect(sel.Filename, worker, false)
	worker.settings.room.broadcastStartOnReady(worker)
}

func (worker *Worker) handlePlaylist(playlist *Playlist) {
	worker.settings.room.changePlaylist(playlist.Playlist, worker)
	worker.settings.room.broadcastPlaylist(playlist, worker, true)
}

func (worker *Worker) handlePause(pause *Pause) {
	worker.settings.room.setPaused(true)
	worker.settings.room.broadcastPause(worker)
}

func (worker *Worker) handlePlaybackSpeed(speed *PlaybackSpeed) {
	worker.updateSpeed(speed.Speed)
	worker.settings.room.updateSpeed(speed.Speed)
	worker.settings.room.broadcastPlaybackSpeed(speed.Speed, worker)
}

func (worker *Worker) login() {
	worker.settingsMutex.Lock()
	defer worker.settingsMutex.Unlock()

	worker.settings.loggedIn = true
}

func (worker *Worker) isLoggedIn() bool {
	worker.settingsMutex.RLock()
	defer worker.settingsMutex.RUnlock()

	return worker.settings.loggedIn
}

func (worker *Worker) updateVideoStatus(videoStatus *VideoStatus, arrivalTime time.Time) {
	worker.videoStatusMutex.Lock()
	defer worker.videoStatusMutex.Unlock()

	worker.videoStatus.filename = videoStatus.Filename
	worker.videoStatus.position = videoStatus.Position
	worker.videoStatus.timestamp = arrivalTime.Add(time.Duration(-worker.latency.roundTripTime/2) * time.Nanosecond)
	worker.videoStatus.paused = videoStatus.Paused
}

func (worker *Worker) updateSpeed(speed float64) {
	worker.videoStatusMutex.Lock()
	defer worker.videoStatusMutex.Unlock()

	worker.videoStatus.speed = speed
}

func (worker *Worker) updateRoom(room *Room) {
	worker.settingsMutex.Lock()
	defer worker.settingsMutex.Unlock()

	worker.settings.room = room
}

func (worker *Worker) sendMessage(message []byte) {
	err := wsutil.WriteServerMessage(worker.settings.conn, ws.OpText, message)
	if err != nil {
		logger.Errorw("Unable to send message", "error", err)
		worker.close()
	}
}

func (worker *Worker) sendServerMessage(message string, isError bool) {
	serverMessage := ServerMessage{Message: message, IsError: isError}
	data, err := MarshalMessage(serverMessage)
	if err != nil {
		logger.Errorw("Unable to parse server message", "error", err)
	}

	worker.sendMessage(data)
}

func (worker *Worker) sendPlaylist() {
	worker.settingsMutex.RLock()
	worker.settings.room.stateMutex.RLock()
	worker.userStatusMutex.RLock()
	defer worker.settingsMutex.RUnlock()
	defer worker.settings.room.stateMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	playlist := Playlist{Playlist: worker.settings.room.state.playlist, Username: worker.userStatus.Username}
	message, err := MarshalMessage(playlist)
	if err != nil {
		logger.Errorw("Unable to marshal playlist", "error", err)
		return
	}

	worker.sendMessage(message)
}
