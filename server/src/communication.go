package niketsu_server

import (
	"fmt"
	"math"
	"net"
	"net/http"
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
}

type FactoryWorker struct {
	capitalist       *Capitalist
	uuid             uuid.UUID
	conn             net.Conn
	serviceChannel   chan int
	closeOnce        sync.Once
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
	worker.uuid = uuid.New() // can panic
	worker.conn = conn
	worker.serviceChannel = make(chan int)
	worker.userStatus = &Status{Ready: false, Username: userName}
	worker.userStatusMutex = &sync.RWMutex{}
	worker.videoStatus = &Video{filename: filename, position: position}
	worker.videoStatusMutex = &sync.RWMutex{}
	worker.latency = &Latency{rtt: 0, timestamps: make(map[uuid.UUID]time.Time)}
	worker.latencyMutex = &sync.RWMutex{}
	return worker
}

func (worker *FactoryWorker) close() {
	worker.closeOnce.Do(func() {
		logger.Infow("Closing connection", "client", worker.uuid)

		close(worker.serviceChannel)
		worker.capitalist.Delete(worker)
		worker.capitalist.broadcastStatusList(worker)
	})
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

func (worker *FactoryWorker) updateRtt(uuid uuid.UUID, arrivalTime time.Time) {
	worker.latencyMutex.Lock()
	defer worker.latencyMutex.Unlock()

	// calculate new avg rtt
	newRtt := float64(arrivalTime.Sub(worker.latency.timestamps[uuid]))
	worker.latency.rtt = worker.latency.rtt*WEIGHTING_FACTOR + newRtt*(1-WEIGHTING_FACTOR)

	delete(worker.latency.timestamps, uuid) // TODO check and delete missing pings
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

	err = wsutil.WriteServerMessage(worker.conn, ws.OpText, message)
	if err != nil {
		logger.Errorw("Unable to send ping message", "error", err)
		worker.close()
	}
}

func (worker *FactoryWorker) sendMessage(message []byte) {
	err := wsutil.WriteServerMessage(worker.conn, ws.OpText, message)
	if err != nil {
		logger.Errorw("Unable to send message", "error", err)
		worker.close()
	}
}

func (worker *FactoryWorker) sendPlaylist() {
	worker.capitalist.playlistMutex.RLock()
	worker.userStatusMutex.RLock()
	defer worker.capitalist.playlistMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	playlist := Playlist{Playlist: worker.capitalist.playlist.playlist, Username: worker.userStatus.Username}
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
	worker.capitalist.broadcastStartOnReady(worker)
}

func (worker *FactoryWorker) handleVideoStatus(videoStatus *VideoStatus, arrivalTime time.Time) {
	if videoStatus.Filename == nil || videoStatus.Position == nil {
		worker.capitalist.handleNilStatus(videoStatus, worker)
		return
	}

	legit := worker.capitalist.checkValidVideoStatus(videoStatus, worker)
	if legit {
		worker.updateVideoStatus(videoStatus, arrivalTime)
		worker.capitalist.evaluateVideoStatus()
	} else {
		worker.capitalist.sendSeek(worker, true)
	}
}

func (worker *FactoryWorker) handleStart(start *Start) {
	worker.capitalist.broadcastStart(start.Filename, worker)
	worker.capitalist.setPaused(false)
}

func (worker *FactoryWorker) handleSeek(seek *Seek, arrivalTime time.Time) {
	worker.capitalist.changePosition(seek.Position)
	worker.capitalist.updateLastSeek(seek.Position)
	worker.updateVideoStatus(&VideoStatus{Filename: &seek.Filename, Position: &seek.Position, Paused: seek.Paused}, arrivalTime)
	worker.capitalist.broadcastSeek(seek.Filename, seek.Position, worker, true)
}

func (worker *FactoryWorker) handleSelect(sel *Select) {
	worker.capitalist.changeVideo(sel.Filename)
	worker.capitalist.changePosition(0)
	worker.capitalist.updateLastSeek(0)
	worker.capitalist.setPaused(true)
	worker.capitalist.broadcastSelect(sel.Filename, worker, false)
	worker.capitalist.broadcastStartOnReady(worker)
}

func (worker *FactoryWorker) handlePlaylist(playlist *Playlist) {
	worker.capitalist.broadcastPlaylist(playlist, worker, true)
	worker.capitalist.changePlaylist(playlist.Playlist, worker)
}

func (worker *FactoryWorker) handlePause(pause *Pause) {
	worker.capitalist.broadcastPause(pause.Filename, worker)
	worker.capitalist.setPaused(true)
}

func (worker *FactoryWorker) handleMessage(data []byte, arrivalTime time.Time) {
	msg, err := UnmarshalMessage(data)
	if err != nil {
		logger.Errorw("Unable to unmarshal client message", "error", err)
		return
	}

	logger.Debugw("Received message from client", "type", msg.Type(), "message", msg)
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
		worker.capitalist.broadcastUserMessage(msg.Message, worker)
	case PlaylistType:
		msg := msg.(*Playlist)
		worker.handlePlaylist(msg)
	case PauseType:
		msg := msg.(*Pause)
		worker.handlePause(msg)
	default:
		logger.Warn("Unknown message handling is not supported.")
	}
}

func (worker *FactoryWorker) Start() {
	defer worker.conn.Close()

	// send client current state
	go worker.HandlerService()
	go worker.sendPlaylist()
	go worker.capitalist.sendSeek(worker, true)

	go worker.PingService()
	<-worker.serviceChannel
}

func (worker *FactoryWorker) HandlerService() {
	for {
		select {
		case <-worker.serviceChannel:
			return
		default:
			data, _, err := wsutil.ReadClientData(worker.conn)
			arrivalTime := time.Now()
			if err != nil {
				logger.Errorw("Unable to read from client", "error", err, "worker", worker.uuid)
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
		case <-worker.serviceChannel:
			return
		case <-ticker.C:
			worker.sendPing()
		}
	}
}

type CapitalistPlaylist struct {
	playlist []string
	video    *string
	position *uint64
	lastSeek uint64
	paused   bool
	saveFile string
}

type CapitalistConfig struct {
	host string
	port uint16
}

type Capitalist struct {
	config        *CapitalistConfig
	playlist      *CapitalistPlaylist
	workers       []*FactoryWorker
	workersMutex  *sync.RWMutex
	playlistMutex *sync.RWMutex
}

func NewCapitalist(host string, port uint16, playlist []string, currentVideo *string, position *uint64, saveFile string) Capitalist {
	var capitalist Capitalist
	capitalist.config = &CapitalistConfig{host: host, port: port}
	capitalist.playlist = &CapitalistPlaylist{playlist: playlist, video: currentVideo, position: position, paused: true, saveFile: saveFile}
	capitalist.workers = make([]*FactoryWorker, 0)
	capitalist.workersMutex = &sync.RWMutex{}
	capitalist.playlistMutex = &sync.RWMutex{}
	return capitalist
}

func (capitalist *Capitalist) Start() {
	logger.Info("Finished initializing manager. Starting http listener ...")

	hostPort := fmt.Sprintf("%s:%d", capitalist.config.host, capitalist.config.port)
	http.ListenAndServe(hostPort, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, _, _, err := ws.UpgradeHTTP(r, w)
		if err != nil {
			logger.Errorw("Failed to establish connection to client socket", "error", err)
		}

		logger.Info("New connection established. Creating new worker ...")
		capitalist.createNewWorker(conn)
	}))
}

func (capitalist *Capitalist) Append(worker *FactoryWorker) {
	capitalist.workersMutex.Lock()
	defer capitalist.workersMutex.Unlock()
	capitalist.workers = append(capitalist.workers, worker)
}

func (capitalist *Capitalist) Delete(worker *FactoryWorker) {
	capitalist.workersMutex.Lock()
	defer capitalist.workersMutex.Unlock()

	// search and destroy
	for i, w := range capitalist.workers {
		if w.uuid == worker.uuid {
			capitalist.workers = append(capitalist.workers[:i], capitalist.workers[i+1:]...)
		}
	}

	// pause video if no clients are connected
	if len(capitalist.workers) == 0 {
		capitalist.setPaused(true)
	}
}

func (capitalist *Capitalist) createNewWorker(conn net.Conn) {
	capitalist.playlistMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()

	worker := NewFactoryWorker(capitalist, conn, "server", capitalist.playlist.video, capitalist.playlist.position)
	capitalist.Append(&worker)

	logger.Infow("Starting new worker for client", "client", worker.uuid)
	go worker.Start()
}

func (capitalist *Capitalist) statusList() []Status {
	capitalist.workersMutex.RLock()
	defer capitalist.workersMutex.RUnlock()

	users := []Status{}
	for _, w := range capitalist.workers {
		w.userStatusMutex.RLock()
		user := Status{Ready: w.userStatus.Ready, Username: w.userStatus.Username}
		users = append(users, user)
		w.userStatusMutex.RUnlock()
	}

	return users
}

func (capitalist *Capitalist) broadcastExcept(message []byte, worker *FactoryWorker) {
	capitalist.workersMutex.RLock()
	defer capitalist.workersMutex.RUnlock()

	for _, w := range capitalist.workers {
		if w.uuid != worker.uuid {
			w.sendMessage(message)
		}
	}
}

func (capitalist *Capitalist) broadcastAll(message []byte) {
	capitalist.workersMutex.RLock()
	defer capitalist.workersMutex.RUnlock()

	for _, w := range capitalist.workers {
		w.sendMessage(message)
	}
}

// StatusList is also sent to the client who sent the last VideoStatus
func (capitalist *Capitalist) broadcastStatusList(worker *FactoryWorker) {
	users := capitalist.statusList()

	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	statusList := StatusList{Users: users, Username: worker.userStatus.Username}
	message, err := statusList.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to parse status list", "error", err)
		return
	}

	capitalist.broadcastAll(message)
}

func (capitalist *Capitalist) broadcastStart(filename string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Start{Filename: filename, Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal start message", "error", err)
		return
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastSeek(filename string, position uint64, worker *FactoryWorker, lock bool) {
	if lock {
		capitalist.playlistMutex.RLock()
		defer capitalist.playlistMutex.RUnlock()
	}

	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Seek{Filename: filename, Position: position, Paused: capitalist.playlist.paused, Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast seek", "error", err)
		return
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastSelect(filename *string, worker *FactoryWorker, all bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Select{Filename: filename, Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast select", "error", err)
		return
	}

	if all {
		capitalist.broadcastAll(message)
	} else {
		capitalist.broadcastExcept(message, worker)
	}
}

func (capitalist *Capitalist) broadcastUserMessage(userMessage string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	um := UserMessage{Message: userMessage, Username: worker.userStatus.Username}
	message, err := um.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast user message", "error", err)
		return
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastPlaylist(playlist *Playlist, worker *FactoryWorker, all bool) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	pl := Playlist{Playlist: playlist.Playlist, Username: worker.userStatus.Username}
	message, err := pl.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast playlist", "error", err)
		return
	}

	if all {
		capitalist.broadcastAll(message)
	} else {
		capitalist.broadcastExcept(message, worker)
	}
}

func (capitalist *Capitalist) broadcastPause(filename string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	p := Pause{Filename: filename, Username: worker.userStatus.Username}
	message, err := p.MarshalMessage()
	if err != nil {
		logger.Errorw("Unable to marshal broadcast pause", "error", err)
		return
	}

	capitalist.broadcastExcept(message, worker)
}

// set paused to false since video will start
func (capitalist *Capitalist) broadcastStartOnReady(worker *FactoryWorker) {
	capitalist.workersMutex.RLock()
	defer capitalist.workersMutex.RUnlock()

	// cannot start nil video
	if capitalist.playlist.video == nil {
		return
	}

	ready := true
	for _, w := range capitalist.workers {
		w.userStatusMutex.RLock()
		ready = ready && w.userStatus.Ready
		w.userStatusMutex.RUnlock()
	}

	if ready {
		capitalist.playlistMutex.Lock()
		worker.userStatusMutex.RLock()
		defer capitalist.playlistMutex.Unlock()
		defer worker.userStatusMutex.RUnlock()

		start := Start{Filename: *capitalist.playlist.video, Username: worker.userStatus.Username}
		message, err := start.MarshalMessage()
		if err != nil {
			logger.Errorw("Unable to marshal start message", "error", err)
			return
		}

		for _, w := range capitalist.workers {
			w.sendMessage(message)
		}

		capitalist.playlist.paused = false
	}
}

func (capitalist *Capitalist) sendSeek(worker *FactoryWorker, lock bool) {
	if lock {
		capitalist.playlistMutex.RLock()
		worker.userStatusMutex.RLock()
		defer capitalist.playlistMutex.RUnlock()
		defer worker.userStatusMutex.RUnlock()
	}

	// seeking nil videos is prohibited
	if capitalist.playlist.video == nil {
		return
	}

	seek := Seek{Filename: *capitalist.playlist.video, Position: *capitalist.playlist.position, Paused: capitalist.playlist.paused, Username: worker.userStatus.Username}
	message, err := seek.MarshalMessage()
	if err != nil {
		logger.Errorw("Capitalist failed to marshal seek", "error", err)
		return
	}

	worker.sendMessage(message)
}

// Evaluates the video states of all clients and broadcasts seek if difference between
// fastest and slowest clients is too large.
func (capitalist *Capitalist) evaluateVideoStatus() {
	capitalist.workersMutex.RLock()
	capitalist.playlistMutex.Lock()
	defer capitalist.workersMutex.RUnlock()
	defer capitalist.playlistMutex.Unlock()

	var slowest *FactoryWorker
	minPosition := uint64(math.MaxUint64)
	maxPosition := uint64(0)

	for _, w := range capitalist.workers {
		w.videoStatusMutex.RLock()

		timeElapsed := uint64(time.Since(w.videoStatus.timestamp).Milliseconds())
		estimatedPosition := timeElapsed + *w.videoStatus.position

		if estimatedPosition < minPosition {
			minPosition = estimatedPosition
			slowest = w
		}

		if estimatedPosition > maxPosition {
			maxPosition = estimatedPosition
		}

		w.videoStatusMutex.RUnlock()
	}

	if minPosition > capitalist.playlist.lastSeek {
		capitalist.playlist.position = &minPosition
	} else {
		capitalist.playlist.position = &capitalist.playlist.lastSeek
	}

	if maxPosition-minPosition > MAX_DIFFERENCE_MILLISECONDS {
		capitalist.broadcastSeek(*capitalist.playlist.video, *capitalist.playlist.position, slowest, false)
	}

	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) findNext(newPlaylist []string) string {
	j := 0

	for _, video := range capitalist.playlist.playlist {
		if video == *capitalist.playlist.video {
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

func (capitalist *Capitalist) changePlaylist(playlist []string, worker *FactoryWorker) {
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	if len(playlist) != 0 && len(playlist) < len(capitalist.playlist.playlist) {
		nextVideo := capitalist.findNext(playlist)
		if nextVideo != *capitalist.playlist.video {
			capitalist.playlist.video = &nextVideo
			capitalist.broadcastSelect(capitalist.playlist.video, worker, true)
		}
	}

	capitalist.playlist.playlist = playlist

	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) changeVideo(fileName *string) {
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	capitalist.playlist.video = fileName
	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) changePosition(position uint64) {
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	capitalist.playlist.position = &position
	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) updateLastSeek(position uint64) {
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	capitalist.playlist.lastSeek = position
}

func (capitalist *Capitalist) checkValidVideoStatus(videoStatus *VideoStatus, worker *FactoryWorker) bool {
	capitalist.playlistMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()

	// video status is not compatible with server if position is not in accordance with the last seek or video is paused when it is not supposed to be
	if *videoStatus.Position < capitalist.playlist.lastSeek || videoStatus.Paused != capitalist.playlist.paused {
		return false
	}

	return true
}

func (capitalist *Capitalist) setPaused(paused bool) {
	capitalist.playlistMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()

	capitalist.playlist.paused = paused
}

func (capitalist *Capitalist) handleNilStatus(videoStatus *VideoStatus, worker *FactoryWorker) {
	capitalist.playlistMutex.RLock()
	worker.userStatusMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	if videoStatus.Filename != capitalist.playlist.video || videoStatus.Position != capitalist.playlist.position {
		capitalist.sendSeek(worker, false)
	}
}
