package niketsu_server

import (
	"fmt"
	"log"
	"math"
	"net"
	"net/http"
	"sync"
	"time"

	"github.com/gobwas/ws"
	"github.com/gobwas/ws/wsutil"
	"github.com/google/uuid"
)

const (
	WEIGHTING_FACTOR            float64       = 0.85
	TICK_INTERVALS              time.Duration = time.Second
	MAX_DIFFERENCE_MILLISECONDS uint64        = 1e3 // one second
	UNSTABLE_LATENCY_THRESHOLD  float64       = 2e3
)

//TODO check userstatus mutexes
//TODO consider rtt in seek broadcast
//TODO consider throwing away lost pings, ...

type Latency struct {
	rtt        float64
	timestamps map[uuid.UUID]time.Time
}

type Video struct {
	filename  string
	position  uint64
	timestamp time.Time
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

func NewFactoryWorker(capitalist *Capitalist, conn net.Conn, userName string, filename string, position uint64) FactoryWorker {
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
	logger.Debug("Closing connection")

	worker.closeOnce.Do(func() {
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
		logger.Warn("Unable to parse ping message")
		return
	}

	worker.latencyMutex.Lock()
	worker.latency.timestamps[uuid] = time.Now()
	worker.latencyMutex.Unlock()

	err = wsutil.WriteServerMessage(worker.conn, ws.OpText, message)
	if err != nil {
		logger.Error("Unable to send ping message")
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

func (worker *FactoryWorker) sendUnsupportedMessage() {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	unsupported := Unsupported{Username: worker.userStatus.Username}
	message, err := unsupported.MarshalMessage()
	if err != nil {
		logger.Warn("Unable to marhsal unsupported message")
		return
	}

	worker.sendMessage(message)
}

func (worker *FactoryWorker) sendPlaylist() {
	defer logger.Debugw("Successfully leaving sendPlaylist")
	worker.capitalist.playlistMutex.RLock()
	worker.userStatusMutex.RLock()
	defer worker.capitalist.playlistMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	playlist := Playlist{Playlist: worker.capitalist.playlist.playlist, Username: worker.userStatus.Username}
	message, err := playlist.MarshalMessage()
	if err != nil {
		logger.Warn("Unable to marshal playlist")
		return
	}

	worker.sendMessage(message)
}

func (worker *FactoryWorker) handleVideoStatus(videoStatus *VideoStatus, arrivalTime time.Time) {
	legit := worker.capitalist.checkValidVideoStatus(videoStatus, worker)

	logger.Debugw("Checked status", "status", legit)
	if legit {
		worker.updateVideoStatus(videoStatus, arrivalTime)
		worker.capitalist.evaluateVideoStatus()
	} else {
		worker.capitalist.sendSeek(worker)
	}
}

func (worker *FactoryWorker) handleMessage(data []byte, op ws.OpCode, err error, arrivalTime time.Time) {
	if err != nil {
		logger.Error("Unable to read from client")
		worker.close()
		return
	}
	//TODO handle different op code

	msg, err := UnmarshalMessage(data)
	if err != nil {
		logger.Error("Unable to unmarshal client message")
		return
	}

	// TODO update worker and capitalist video status
	switch msg.Type() {
	case PingType:
		//logger.Debugw("Received ping from client", "message", msg)

		uuid, err := uuid.Parse(msg.(*Ping).Uuid)
		if err != nil {
			logger.Warn("Unable to parse uuid")
			return
		}
		worker.updateRtt(uuid, arrivalTime)
	case StatusType:
		logger.Debugw("Received status from client", "message", msg)

		msg := msg.(*Status)
		worker.updateUserStatus(msg)
		worker.capitalist.broadcastStatusList(worker)
		worker.capitalist.broadcastStartOnReady(worker)
	case VideoStatusType:
		logger.Debugw("Received video status from client", "message", msg)

		msg := msg.(*VideoStatus)
		worker.handleVideoStatus(msg, arrivalTime)
	case StartType:
		logger.Debugw("Received start from client", "message", msg)

		msg := msg.(*Start)
		worker.updateUserStatus(&Status{Ready: true, Username: worker.userStatus.Username}) // user is ready for playing
		worker.capitalist.broadcastStart(msg.Filename, worker)
		worker.capitalist.setPaused(false)
	case SeekType:
		logger.Debugw("Received seek from client", "message", msg)

		msg := msg.(*Seek)
		worker.capitalist.changePosition(msg.Position)
		worker.capitalist.updateLastSeek(msg.Position)

		worker.updateVideoStatus(&VideoStatus{Filename: msg.Filename, Position: msg.Position, Paused: msg.Paused}, arrivalTime)
		worker.capitalist.broadcastSeek(msg.Filename, msg.Position, worker, true)
	case SelectType:
		logger.Debugw("Received select from client", "message", msg)

		msg := msg.(*Select)
		// TODO only send if video is not current
		worker.capitalist.changeVideo(msg.Filename)
		worker.capitalist.changePosition(0)
		worker.capitalist.updateLastSeek(0)
		worker.capitalist.broadcastSelect(msg.Filename, worker)
		worker.capitalist.broadcastStartOnReady(worker)
	case UserMessageType:
		logger.Debugw("Received user message from client", "message", msg)

		msg := msg.(*UserMessage)
		worker.capitalist.broadcastUserMessage(msg.Message, worker)
	case PlaylistType:
		logger.Debugw("Received playlist from client", "message", msg)

		msg := msg.(*Playlist)
		worker.capitalist.broadcastPlaylist(worker)
		worker.capitalist.changePlaylist(msg.Playlist, worker)
	case PauseType:
		logger.Debugw("Received pause from client", "message", msg)

		msg := msg.(*Pause)
		worker.capitalist.broadcastPause(msg.Filename, worker)
		worker.capitalist.setPaused(true)
	default:
		logger.Infow("Received unknown message from client", "message", msg)
	}
}

func (worker *FactoryWorker) Start() {
	defer worker.conn.Close()

	// send client current state
	go worker.HandlerService()
	go worker.sendPlaylist()
	go worker.capitalist.sendSeek(worker)

	go worker.PingService()
	<-worker.serviceChannel
}

func (worker *FactoryWorker) HandlerService() {
	for {
		select {
		case <-worker.serviceChannel:
			return
		default:
			data, op, err := wsutil.ReadClientData(worker.conn)
			arrivalTime := time.Now()
			go worker.handleMessage(data, op, err, arrivalTime)
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
	video    string
	position uint64
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

func NewCapitalist(host string, port uint16, playlist []string, currentVideo string, position uint64, saveFile string) Capitalist {
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
			// handle error
			fmt.Println("Connection error", err)
		}
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
	defer logger.Debugw("Successfully leaving createNewWorker")
	capitalist.playlistMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()

	worker := NewFactoryWorker(capitalist, conn, "server", capitalist.playlist.video, capitalist.playlist.position)
	capitalist.Append(&worker)
	go worker.Start()
}

func (capitalist *Capitalist) statusList() []Status {
	capitalist.workersMutex.RLock()
	defer capitalist.workersMutex.RUnlock()

	users := []Status{}
	for _, w := range capitalist.workers {
		// put lock outside of loop?
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
		logger.Warn("Unable to parse status list")
	}

	capitalist.broadcastAll(message)
}

func (capitalist *Capitalist) broadcastStart(filename string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Start{Filename: filename, Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		logger.Warn("Unable to parse start message")
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastSeek(filename string, position uint64, worker *FactoryWorker, lock bool) {
	defer logger.Debugw("Successfully leaving broadcastSeek")
	if lock {
		capitalist.playlistMutex.RLock()
		defer capitalist.playlistMutex.RUnlock()
	}

	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Seek{Filename: filename, Position: position, Paused: capitalist.playlist.paused, Username: worker.userStatus.Username}
	logger.Debugw("Sven sagt, dass ich die Variable s an dieser Stelle loggen soll", "s", s)
	message, err := s.MarshalMessage()
	if err != nil {
		log.Fatal("unable to broadcast seek ", err)
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastSelect(filename string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	s := Select{Filename: filename, Username: worker.userStatus.Username}
	message, err := s.MarshalMessage()
	if err != nil {
		log.Fatal("unable to broadcast select ", err)
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastUserMessage(userMessage string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	um := UserMessage{Message: userMessage, Username: worker.userStatus.Username}
	message, err := um.MarshalMessage()
	if err != nil {
		log.Fatal("unable to broadcast user message ", err)
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastPlaylist(worker *FactoryWorker) {
	defer logger.Debugw("Successfully leaving broadcastPlaylist")
	capitalist.playlistMutex.RLock()
	worker.userStatusMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()
	defer worker.userStatusMutex.RUnlock()

	playlist := Playlist{Playlist: capitalist.playlist.playlist, Username: worker.userStatus.Username}
	message, err := playlist.MarshalMessage()
	if err != nil {
		log.Fatal("unable to broadcast playlist ", err)
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastPause(filename string, worker *FactoryWorker) {
	worker.userStatusMutex.RLock()
	defer worker.userStatusMutex.RUnlock()

	p := Pause{Filename: filename, Username: worker.userStatus.Username}
	message, err := p.MarshalMessage()
	if err != nil {
		log.Fatal("unable to broadcast pause ", err)
	}

	capitalist.broadcastExcept(message, worker)
}

func (capitalist *Capitalist) broadcastStartOnReady(worker *FactoryWorker) {
	defer logger.Debugw("Successfully leaving broadcastonready stuff")
	capitalist.workersMutex.RLock()
	defer capitalist.workersMutex.RUnlock()

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

		start := Start{Filename: capitalist.playlist.video, Username: worker.userStatus.Username}
		message, err := start.MarshalMessage()
		if err != nil {
			logger.Warnf("Failed to marshal start message", "error", err)
		}

		for _, w := range capitalist.workers {
			w.sendMessage(message)
		}

		// set paused to false since video will start
		capitalist.playlist.paused = false
		logger.Debugw("Ready")
	}
}

func (capitalist *Capitalist) sendSeek(worker *FactoryWorker) {
	defer logger.Debugw("Successfully leaving sendseek")
	capitalist.playlistMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()

	logger.Debugw("Status of server when player joining", "file", capitalist.playlist.video, "position", capitalist.playlist.position, "paused", capitalist.playlist.paused)
	seek := Seek{Filename: capitalist.playlist.video, Position: capitalist.playlist.position, Paused: capitalist.playlist.paused, Username: worker.userStatus.Username}
	message, err := seek.MarshalMessage()
	if err != nil {
		logger.Warnw("Capitalist failed to marshal seek", "error", err)
	}

	worker.sendMessage(message)
}

// Evaluates the video states of all clients and broadcasts seek if difference between
// fastest and slowest clients is too large.
func (capitalist *Capitalist) evaluateVideoStatus() {
	defer logger.Debugw("Successfully leaving evaluatevideostatus")
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
		estimatedPosition := timeElapsed + w.videoStatus.position

		if estimatedPosition < minPosition {
			minPosition = estimatedPosition
			slowest = w
		}

		if estimatedPosition > maxPosition {
			maxPosition = estimatedPosition
		}

		w.videoStatusMutex.RUnlock()
	}

	logger.Debugw("Worker times", "slowest", minPosition, "fastest", maxPosition)

	if minPosition > capitalist.playlist.lastSeek {
		capitalist.playlist.position = minPosition
	} else {
		capitalist.playlist.position = capitalist.playlist.lastSeek
	}

	if maxPosition-minPosition > MAX_DIFFERENCE_MILLISECONDS {
		logger.Debugw("Broadcasting seek since time difference too large", "difference", maxPosition-minPosition)
		capitalist.broadcastSeek(capitalist.playlist.video, capitalist.playlist.position, slowest, false)
	}

	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) findNext(newPlaylist []string) string {
	j := 0

	for _, video := range capitalist.playlist.playlist {
		if video == capitalist.playlist.video {
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
	defer logger.Debugw("Successfully leaving changePlaylist")
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	if len(playlist) >= len(capitalist.playlist.playlist) {
		capitalist.playlist.playlist = playlist
		return
	}

	if len(playlist) < len(capitalist.playlist.playlist) {
		capitalist.playlist.video = capitalist.findNext(playlist)
		capitalist.broadcastSelect(capitalist.playlist.video, worker)
	}

	capitalist.playlist.playlist = playlist
	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) changeVideo(fileName string) {
	defer logger.Debugw("Successfully leaving changeVideo")
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	capitalist.playlist.video = fileName
	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) changePosition(position uint64) {
	defer logger.Debugw("Successfully leaving changePosition")
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	capitalist.playlist.position = position
	go WritePlaylist(capitalist.playlist.playlist, capitalist.playlist.video, capitalist.playlist.position, capitalist.playlist.saveFile)
}

func (capitalist *Capitalist) updateLastSeek(position uint64) {
	capitalist.playlistMutex.Lock()
	defer capitalist.playlistMutex.Unlock()

	capitalist.playlist.lastSeek = position
}

func (capitalist *Capitalist) checkValidVideoStatus(videoStatus *VideoStatus, worker *FactoryWorker) bool {
	defer logger.Debugw("Successfully leaving checkValidVideoStatus")
	capitalist.playlistMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()

	// video status is not compatible with server if position is not in accordance with the last seek or video is paused when it is not supposed to be
	logger.Debugw("Videostatus", "client", videoStatus, "server_paused", capitalist.playlist.paused, "server_position", capitalist.playlist.position, "lastseek", capitalist.playlist.lastSeek)
	if videoStatus.Position < capitalist.playlist.lastSeek || videoStatus.Paused != capitalist.playlist.paused {
		return false
	}

	return true
}

func (capitalist *Capitalist) setPaused(paused bool) {
	defer logger.Debugw("Successfully leaving setPaused")
	capitalist.playlistMutex.RLock()
	defer capitalist.playlistMutex.RUnlock()

	capitalist.playlist.paused = paused
}
