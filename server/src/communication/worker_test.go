package communication

import (
	"encoding/json"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/golang/mock/gomock"
	"github.com/google/uuid"
	"github.com/stretchr/testify/require"
)

const (
	username           string        = "testUser"
	notReady           bool          = false
	defaultWorkerSpeed float64       = 1.0
	messageDelay       time.Duration = 10 * time.Millisecond
)

var (
	simplePayload     = []byte("test payload")
	sampleArrivalTime = time.Date(2023, 4, 20, 6, 9, 2, 0, time.Local)
	simpleTask        = Task{payload: simplePayload, arrivalTime: sampleArrivalTime}
	simpleMessage     = []byte("test message")
	simpleStatus      = Status{Username: username, Ready: notReady}
	emptyStatus       = Status{}
	simpleVideoState  = &workerVideoState{
		video:     &testWorkerVideo,
		position:  &testWorkerPosition,
		timestamp: closeDate,
		paused:    notPaused,
		speed:     defaultWorkerSpeed,
	}
	testWorkerVideo    string = "testVideo"
	testWorkerVideo2   string = "testVideo2"
	testWorkerPosition uint64 = 100
	emptyVideoState           = &workerVideoState{}
	simpleVideoStatus         = VideoStatus{
		Filename: &testWorkerVideo,
		Position: &testWorkerPosition,
		Paused:   notReady,
		Speed:    defaultWorkerSpeed,
	}
	simplePlaylist  = []string{testWorkerVideo, testWorkerVideo2}
	simpleRoomState = RoomState{
		playlist: simplePlaylist,
		video:    &testWorkerVideo,
		position: &testWorkerPosition,
		lastSeek: 0,
		paused:   notPaused,
		speed:    1.0,
	}
	emptyRoomState             = RoomState{}
	workerUUIDS    []uuid.UUID = []uuid.UUID{
		uuid.Must(uuid.Parse("123e4567-e89b-12d3-a456-426655440000")),
		uuid.Must(uuid.Parse("123e4567-e89b-12d3-a456-426655440001")),
		uuid.Must(uuid.Parse("123e4567-e89b-12d3-a456-426655440002")),
	}
	closeDate   = time.Now()
	farDate     = time.Date(2023, 1, 1, 1, 1, 1, 1, time.Local)
	defaultUUID = uuid.Must(uuid.Parse("00000000-0000-0000-0000-000000000000"))
)

func TestNewWorker(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoomHandler := NewMockServerStateHandler(ctrl)
	mockWebsocket := NewMockWebsocketReaderWriter(ctrl)
	worker := NewWorker(mockRoomHandler, mockWebsocket, username)

	require.NotNil(t, worker)
	testCorrectWorkerState(t, worker.(*Worker))
}

func testCorrectWorkerState(t *testing.T, worker *Worker) {
	require.NotNil(t, worker.roomHandler)
	require.NotNil(t, worker.websocket)

	require.NotNil(t, worker.state)
	require.NotNil(t, worker.state.uuid)
	require.Nil(t, worker.room)
	require.NotNil(t, worker.state.loggedIn)
	require.Empty(t, worker.state.stopChan)
	require.NotNil(t, worker.state.closeOnce)

	require.NotNil(t, worker.userStatus)
	userStatus := worker.userStatus
	require.False(t, userStatus.Ready)
	require.Equal(t, username, worker.userStatus.Username)

	require.NotNil(t, worker.videoState)
	require.Empty(t, worker.videoState.video)
	require.Empty(t, worker.videoState.position)
	require.Empty(t, worker.videoState.timestamp)
	require.True(t, worker.videoState.paused)
	require.Equal(t, 1.0, worker.videoState.speed)

	require.NotNil(t, worker.latency)
	require.Empty(t, worker.latency.roundTripTime)
	require.Empty(t, worker.latency.timestamps)
	require.NotNil(t, worker.latencyMutex)
}

func TestWorkerStartClose(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWebsocket := setUpMockWebsocket(ctrl, nil)
	worker := &Worker{
		websocket: mockWebsocket,
		state: workerState{
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
		latencyMutex: &sync.RWMutex{},
	}

	startCloseWorker(t, worker)
}

func setUpMockWebsocket(ctrl *gomock.Controller, readMessage []byte) *MockWebsocketReaderWriter {
	mockWebsocket := NewMockWebsocketReaderWriter(ctrl)
	mockWebsocket.EXPECT().
		ReadMessage().
		DoAndReturn(func() ([]byte, error) {
			time.Sleep(messageDelay)
			return readMessage, nil
		}).
		AnyTimes()

	mockWebsocket.EXPECT().
		WriteMessage(gomock.Any()).
		DoAndReturn(func(payload []byte) error {
			return nil
		}).
		AnyTimes()

	mockWebsocket.EXPECT().
		Close().
		Return(nil).
		Times(1)

	return mockWebsocket
}

func startCloseWorker(t *testing.T, worker ClientWorker) {
	var wg sync.WaitGroup
	wg.Add(1)
	go startWorker(t, worker, &wg)
	time.Sleep(200 * time.Millisecond)
	worker.Close()
	wg.Wait()
}

func startWorker(t *testing.T, worker ClientWorker, wg *sync.WaitGroup) {
	worker.Start()
	t.Failed()
	wg.Done()
}

func TestWorkerStartCloseWithRoom(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWebsocket := setUpMockWebsocket(ctrl, nil)
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)

	worker := &Worker{
		websocket:   mockWebsocket,
		room:        mockRoom,
		roomHandler: mockRoomHandler,
		state: workerState{
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
		latencyMutex: &sync.RWMutex{},
	}

	startCloseWorker(t, worker)
}

func setUpMockRoom(ctrl *gomock.Controller, roomState RoomState, allUsersReady bool, shouldBeClosed bool) *MockRoomStateHandler {
	mockRoom := NewMockRoomStateHandler(ctrl)

	mockRoom.EXPECT().
		RoomState().
		Return(&roomState).
		AnyTimes()

	mockRoom.EXPECT().
		AllUsersReady().
		Return(allUsersReady).
		AnyTimes()

	mockRoom.EXPECT().
		SetPaused(gomock.Any()).
		AnyTimes()

	mockRoom.EXPECT().
		DeleteWorker(gomock.Any()).
		Times(1)

	mockRoom.EXPECT().
		ShouldBeClosed().
		Return(shouldBeClosed).
		Times(1)

	return mockRoom
}

func setUpMockRoomHandler(ctrl *gomock.Controller, expectedBroadcasts int, expectedDeleteRoom int) *MockServerStateHandler {
	mockRoomHandler := NewMockServerStateHandler(ctrl)

	mockRoomHandler.EXPECT().
		BroadcastStatusList().
		Times(expectedBroadcasts)

	mockRoomHandler.EXPECT().
		DeleteRoom(gomock.Any()).
		Times(expectedDeleteRoom)

	return mockRoomHandler
}

func TestCloseBeforeStart(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWebsocket := setUpMockWebsocket(ctrl, nil)
	worker := &Worker{
		websocket: mockWebsocket,
		state: workerState{
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
		latencyMutex: &sync.RWMutex{},
	}

	worker.Close()
	startCloseWorker(t, worker)
}

func TestShutdown(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWebsocket := setUpMockWebsocket(ctrl, nil)
	worker := &Worker{
		websocket: mockWebsocket,
		state: workerState{
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
		latencyMutex: &sync.RWMutex{},
	}

	startShutdownWorker(t, worker)
}

func startShutdownWorker(t *testing.T, worker ClientWorker) {
	var wg sync.WaitGroup
	wg.Add(1)
	go startWorker(t, worker, &wg)
	time.Sleep(200 * time.Millisecond)
	worker.Shutdown()
	wg.Wait()
}

func TestHandlePing(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	ping := []byte(fmt.Sprintf(`{"uuid":"%s","type":"ping"}`, workerUUIDS[0].String()))
	timestamps := map[uuid.UUID]time.Time{
		workerUUIDS[0]: farDate,
	}
	mockWebsocket := setUpMockWebsocket(ctrl, ping)

	worker := &Worker{
		websocket: mockWebsocket,
		state: workerState{
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		latency:      &workerLatency{timestamps: timestamps},
		latencyMutex: &sync.RWMutex{},
	}

	startCloseWorker(t, worker)
	require.NotEmpty(t, worker.latency.roundTripTime)
}

func TestHandleStatus(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	status := []byte(fmt.Sprintf(`{"ready":%t,"username":"%s","type":"status"}`, notReady, username))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 2, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, status)

	mockRoom.EXPECT().
		SetWorkerStatus(gomock.Eq(defaultUUID), gomock.Eq(Status{Ready: notReady, Username: username})).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		userStatus:   Status{Ready: true, Username: ""},
		latencyMutex: &sync.RWMutex{},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
	require.NotEmpty(t, worker.userStatus)
	require.Equal(t, notReady, worker.userStatus.Ready)
	require.Equal(t, username, worker.userStatus.Username)
}

//TODO test all users ready case

func TestHandleVideoStatusEqualStates(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
		testWorkerVideo, testWorkerPosition, notPaused, defaultSpeed, username))

	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus)
	startCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo, *worker.videoState.video)
	require.Equal(t, testWorkerPosition, *worker.videoState.position)
	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func setUpWorkerForVideoStatus(t *testing.T, ctrl *gomock.Controller, videoStatus []byte) *Worker {
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoom.EXPECT().
		SlowestEstimatedClientPosition().
		Return(&testWorkerPosition).
		AnyTimes()

	mockRoom.EXPECT().
		SetPosition(gomock.Eq(testWorkerPosition)).
		AnyTimes()

	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, videoStatus)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:      &workerVideoState{},
		videoStateMutex: &sync.RWMutex{},
		userStatus:      Status{Ready: true, Username: ""},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	return worker
}

func TestHandleVideoStatusIncorrectFilename(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
		testWorkerVideo2, testWorkerPosition, notPaused, defaultSpeed, username))

	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus)
	startCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo2, *worker.videoState.video)
	require.Equal(t, testWorkerPosition, *worker.videoState.position)
	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func TestHandleVideoStatusIncorrectSpeed(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
		testWorkerVideo, testWorkerPosition, notPaused, 2.0, username))

	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus)
	startCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo, *worker.videoState.video)
	require.Equal(t, testWorkerPosition, *worker.videoState.position)
	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, 2.0, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func TestHandleVideoStatusIncorrectPaused(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
		testWorkerVideo, testWorkerPosition, true, defaultSpeed, username))

	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus)
	startCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo, *worker.videoState.video)
	require.Equal(t, testWorkerPosition, *worker.videoState.position)
	require.Equal(t, true, worker.videoState.paused)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func TestHandleStart(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	status := []byte(fmt.Sprintf(`{"username":"%s","type":"start"}`, username))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, status)
	mockRoom.EXPECT().
		SetPaused(gomock.Eq(false)).
		AnyTimes()

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(status), gomock.Eq(defaultUUID)).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:      simpleVideoState,
		videoStateMutex: &sync.RWMutex{},
		userStatus:      Status{Username: username},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
	require.Equal(t, false, worker.videoState.paused)
}

func TestHandleSeek(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	seek := []byte(fmt.Sprintf(`{"filename":"%s","position":%d,"speed":%g,"paused":%t,"desync":%t,"username":"","type":"seek"}`,
		testWorkerVideo, testWorkerPosition, defaultSpeed, notPaused, false))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, seek)

	mockRoom.EXPECT().
		SetPlaylistState(gomock.Any(), gomock.Eq(testWorkerPosition),
			gomock.Eq(notPaused), gomock.Eq(testWorkerPosition), gomock.Eq(defaultSpeed)).
		Do(func(filename *string, position uint64, paused bool, lastSeek uint64, speed float64) {
			require.Equal(t, testWorkerVideo, *filename)
		}).
		AnyTimes()

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(seek), gomock.Eq(defaultUUID)).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:      emptyVideoState,
		videoStateMutex: &sync.RWMutex{},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, testWorkerPosition, *worker.videoState.position)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
	require.Equal(t, testWorkerVideo, *worker.videoState.video)
}

func TestHandleSelect(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	sel := []byte(fmt.Sprintf(`{"filename":"%s","username":"","type":"select"}`, testWorkerVideo))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, sel)

	mockRoom.EXPECT().
		SetPlaylistState(gomock.Any(), gomock.Eq(uint64(0)), gomock.Eq(true), gomock.Eq(uint64(0)), gomock.Eq(float64(-1))).
		Do(func(filename *string, position uint64, paused bool, lastSeek uint64, speed float64) {
			require.Equal(t, testWorkerVideo, *filename)
		}).
		AnyTimes()

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(sel), gomock.Eq(defaultUUID)).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:      &workerVideoState{speed: 1},
		videoStateMutex: &sync.RWMutex{},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
	require.Equal(t, paused, worker.videoState.paused)
	require.Equal(t, uint64(0), *worker.videoState.position)
	require.Equal(t, float64(1), worker.videoState.speed)
	require.Equal(t, testWorkerVideo, *worker.videoState.video)
}

func TestHandleUserMessage(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	userMessage := []byte(fmt.Sprintf(`{"message":"%s","username":"%s","type":"userMessage"}`, testWorkerVideo, username))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, userMessage)

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(userMessage), gomock.Eq(defaultUUID)).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		userStatus:   Status{Username: username},
		latencyMutex: &sync.RWMutex{},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
}

func TestHandlePlaylist(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	jsonPlaylist, err := json.Marshal(simplePlaylist)
	require.NoError(t, err)
	playlist := []byte(fmt.Sprintf(`{"playlist":%s,"username":"","type":"playlist"}`, string(jsonPlaylist)))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, playlist)

	mockRoom.EXPECT().
		BroadcastAll(gomock.Eq(playlist)).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		latencyMutex: &sync.RWMutex{},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
}

//TODO test playlist change

func TestHandlePause(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	pause := []byte(fmt.Sprintf(`{"username":"","type":"pause"}`))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, pause)

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(pause), gomock.Eq(defaultUUID)).
		AnyTimes()

	mockRoom.EXPECT().
		SetPaused(gomock.Eq(true)).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:      &workerVideoState{},
		videoStateMutex: &sync.RWMutex{},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
	require.True(t, worker.videoState.paused)
}

func TestHandleFailedJoin(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	join := []byte(fmt.Sprintf(`{"password":"%s","room":"%s","username":"%s","type":"join"}`, "testPassword", "testRoom", username))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, join)

	mockRoomHandler.EXPECT().
		IsPasswordCorrect(gomock.Eq("testPassword")).
		Return(false).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		videoState:      &workerVideoState{},
		videoStateMutex: &sync.RWMutex{},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
	require.False(t, worker.state.loggedIn)
}

func TestHandleJoin(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	join := []byte(fmt.Sprintf(`{"password":"%s","room":"%s","username":"%s","type":"join"}`, "testPassword", "testRoom", username))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, join)

	mockRoom.EXPECT().
		DeleteWorker(gomock.Eq(defaultUUID)).
		AnyTimes()

	mockRoom.EXPECT().
		ShouldBeClosed().
		Return(true).
		AnyTimes()

	mockRoom.EXPECT().
		Close().
		AnyTimes()

	mockRoom.EXPECT().
		SetWorkerStatus(gomock.Eq(defaultUUID), gomock.Eq(Status{Ready: false, Username: username})).
		AnyTimes()

	mockRoom.EXPECT().
		Start().
		AnyTimes()

	mockRoomHandler.EXPECT().
		CreateOrFindRoom(gomock.Eq("testRoom")).
		Return(mockRoom, nil).
		AnyTimes()

	mockRoomHandler.EXPECT().
		IsPasswordCorrect(gomock.Eq("testPassword")).
		Return(true).
		AnyTimes()

	mockRoomHandler.EXPECT().
		AppendRoom(gomock.Eq(mockRoom)).
		AnyTimes()

	mockRoomHandler.EXPECT().
		DeleteRoom(gomock.Eq(mockRoom)).
		AnyTimes()

	mockRoomHandler.EXPECT().
		BroadcastStatusList().
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		videoState:      &workerVideoState{},
		videoStateMutex: &sync.RWMutex{},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	mockRoom.EXPECT().
		AppendWorker(worker).
		AnyTimes()

	startCloseWorker(t, worker)
	require.True(t, worker.state.loggedIn)
}

func TestHandlePlaybackSpeed(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	playbackSpeed := []byte(fmt.Sprintf(`{"speed":%g,"username":"","type":"playbackSpeed"}`, defaultSpeed))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, playbackSpeed)

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(playbackSpeed), gomock.Eq(defaultUUID)).
		AnyTimes()

	mockRoom.EXPECT().
		SetSpeed(gomock.Eq(defaultSpeed)).
		AnyTimes()

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:      &workerVideoState{},
		videoStateMutex: &sync.RWMutex{},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
}

func TestHandleUnknown(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	unknown := []byte(fmt.Sprintf(`{"someObject":"someText","type":"someType"}`))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, unknown)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			closeOnce: &sync.Once{},
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		latencyMutex: &sync.RWMutex{},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	startCloseWorker(t, worker)
}

func TestSendPing(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	worker := &Worker{
		latencyMutex: &sync.RWMutex{},
		latency:      &workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
		state:        workerState{writeChan: make(chan []byte, 2)},
	}
	require.Empty(t, worker.latency.timestamps)
	worker.sendPing()
	require.NotEmpty(t, worker.latency.timestamps)
	msg := <-worker.state.writeChan
	require.NotEmpty(t, msg)

	var ping map[string]string
	err := json.Unmarshal(msg, &ping)
	require.NoError(t, err)
	require.Equal(t, "ping", ping["type"])
	_, err = uuid.Parse(ping["uuid"])
	require.NoError(t, err)
}

func TestDeletePings(t *testing.T) {
	testDeletePingsSome(t)
	testDeletePingsAll(t)
	testDeletePingsSome(t)
}

func testDeletePingsSome(t *testing.T) {
	worker := &Worker{
		latencyMutex: &sync.RWMutex{},
		latency: &workerLatency{timestamps: map[uuid.UUID]time.Time{
			workerUUIDS[0]: closeDate,
			workerUUIDS[1]: farDate,
		}},
	}
	worker.deletePings()
	require.Len(t, worker.latency.timestamps, 1)
	require.Contains(t, worker.latency.timestamps, workerUUIDS[0])
}

func testDeletePingsAll(t *testing.T) {
	worker := &Worker{
		latencyMutex: &sync.RWMutex{},
		latency: &workerLatency{timestamps: map[uuid.UUID]time.Time{
			workerUUIDS[0]: farDate,
			workerUUIDS[1]: farDate,
		}},
	}
	worker.deletePings()
	require.Len(t, worker.latency.timestamps, 0)
}

func testDeletePingsNone(t *testing.T) {
	worker := &Worker{
		latencyMutex: &sync.RWMutex{},
		latency: &workerLatency{timestamps: map[uuid.UUID]time.Time{
			workerUUIDS[0]: closeDate,
			workerUUIDS[1]: closeDate,
		}},
	}
	worker.deletePings()
	require.Len(t, worker.latency.timestamps, 2)
	require.Contains(t, worker.latency.timestamps, workerUUIDS[0])
	require.Contains(t, worker.latency.timestamps, workerUUIDS[1])

	worker.latency.timestamps = make(map[uuid.UUID]time.Time)
	worker.deletePings()
	require.Empty(t, worker.latency.timestamps)
}

func TestUUID(t *testing.T) {
	worker := Worker{state: workerState{}}

	worker.state.uuid = workerUUIDS[0]
	workerUUID := worker.UUID()
	require.Equal(t, workerUUIDS[0], workerUUID)
}

func TestSetUserStatus(t *testing.T) {
	worker := Worker{}

	worker.SetUserStatus(simpleStatus)
	status := worker.userStatus
	require.Equal(t, simpleStatus, status)

	worker.SetUserStatus(emptyStatus)
	require.Equal(t, emptyStatus, worker.userStatus)
}

func TestVideoState(t *testing.T) {
	worker := Worker{
		videoState:      simpleVideoState,
		videoStateMutex: &sync.RWMutex{},
	}

	worker.videoState = simpleVideoState
	videoState := worker.VideoState()
	require.Equal(t, simpleVideoState, videoState)
}

func TestSetVideoState(t *testing.T) {
	worker := &Worker{videoState: &workerVideoState{},
		latencyMutex:    &sync.RWMutex{},
		latency:         &workerLatency{},
		videoStateMutex: &sync.RWMutex{},
	}

	worker.setVideoState(simpleVideoStatus, sampleArrivalTime)
	require.Equal(t, *simpleVideoStatus.Filename, *worker.videoState.video)
	require.Equal(t, *simpleVideoStatus.Position, *worker.videoState.position)
	require.Equal(t, simpleVideoStatus.Paused, worker.videoState.paused)
	require.Equal(t, sampleArrivalTime, worker.videoState.timestamp)
	require.Equal(t, simpleVideoStatus.Speed, worker.videoState.speed)
}

func TestSendSeek(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)

	mockRoom.EXPECT().
		RoomState().
		Return(&simpleRoomState).
		AnyTimes()

	worker := &Worker{
		room: mockRoom,
		state: workerState{
			writeChan: make(chan []byte, 10),
		},
		videoState:      &workerVideoState{},
		videoStateMutex: &sync.RWMutex{},
		latency:         &workerLatency{},
		latencyMutex:    &sync.RWMutex{},
	}

	worker.sendSeek(true)
	message := <-worker.state.writeChan
	expectedMessage := []byte(fmt.Sprintf(`{"filename":"%s","position":%d,"speed":%g,"paused":%t,"desync":%t,"username":"","type":"seek"}`,
		*simpleRoomState.video, *simpleRoomState.position, simpleRoomState.speed, simpleRoomState.paused, true))

	require.Equal(t, expectedMessage, message)
}

//TODO more seek tests

func TestSendMessage(t *testing.T) {
	worker := &Worker{
		state: workerState{
			writeChan: make(chan []byte, 10),
		},
	}

	message := []byte(`{"some":"message"}`)
	worker.SendMessage(message)
	receivedMessage := <-worker.state.writeChan
	require.Equal(t, message, receivedMessage)
}

func TestSendServerMessage(t *testing.T) {
	worker := &Worker{
		state: workerState{
			writeChan: make(chan []byte, 10),
		},
	}

	message := "testMessage"
	worker.sendServerMessage(message, false)
	receivedMessage := <-worker.state.writeChan
	expectedMessage := []byte(fmt.Sprintf(`{"message":"%s","error":false,"type":"serverMessage"}`, message))

	require.Equal(t, expectedMessage, receivedMessage)

	worker.sendServerMessage(message, true)
	receivedMessage = <-worker.state.writeChan
	expectedMessage = []byte(fmt.Sprintf(`{"message":"%s","error":true,"type":"serverMessage"}`, message))

	require.Equal(t, expectedMessage, receivedMessage)
}

func TestSendPlaylist(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)

	mockRoom.EXPECT().
		RoomState().
		Return(&simpleRoomState).
		AnyTimes()

	worker := &Worker{
		room: mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			writeChan: make(chan []byte, 10),
		},
		userStatus: Status{},
	}

	worker.sendPlaylist()
	receivedMessage := <-worker.state.writeChan
	jsonPlaylist, err := json.Marshal(simplePlaylist)
	require.NoError(t, err)
	expectedMessage := []byte(fmt.Sprintf(`{"playlist":%s,"username":"","type":"playlist"}`, string(jsonPlaylist)))

	require.Equal(t, expectedMessage, receivedMessage)
}

func TestEstimatePosition(t *testing.T) {
	timestamp := farDate
	worker := &Worker{
		videoState:      &workerVideoState{position: nil, timestamp: timestamp, paused: false, speed: defaultSpeed},
		videoStateMutex: &sync.RWMutex{},
	}

	estimatedPosition := worker.EstimatePosition()
	require.Nil(t, estimatedPosition)

	edgePosition := uint64(0)
	worker.videoState.position = &edgePosition
	estimatedPosition = worker.EstimatePosition()
	expectedPosition := uint64(time.Since(timestamp).Milliseconds())
	require.Equal(t, expectedPosition, *estimatedPosition)

	properPosition := uint64(10000)
	worker.videoState.position = &properPosition
	estimatedPosition = worker.EstimatePosition()

	timeElapsed := uint64(float64(time.Since(timestamp).Milliseconds()) * defaultSpeed)
	expectedPosition = properPosition + timeElapsed
	require.Equal(t, expectedPosition, *estimatedPosition)

	worker.videoState.paused = true
	estimatedPosition = worker.EstimatePosition()

	expectedPosition = properPosition
	require.Equal(t, expectedPosition, *estimatedPosition)
}
