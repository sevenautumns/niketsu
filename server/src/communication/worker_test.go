package communication

import (
	"encoding/json"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/stretchr/testify/require"
	gomock "go.uber.org/mock/gomock"
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
	simpleVideoState  = workerVideoState{
		video:     &testWorkerVideo,
		position:  &testWorkerPosition,
		timestamp: closeDate,
		paused:    notPaused,
		speed:     defaultWorkerSpeed,
	}
	testWorkerVideo               string   = "testVideo"
	testWorkerVideo2              string   = "testVideo2"
	testWorkerPosition            Duration = Duration{100}
	testWorkerPositionMillis      uint64   = 100
	testWorkerPositionMillisecond Duration = Duration{100 * 1000000}
	testWorkerDuration            Duration = Duration{100000 * time.Millisecond}
	testWorkerDurationMillis      uint64   = 100000
	testWorkerCache               Duration = Duration{1000 * time.Millisecond}
	testWorkerCacheMillis         uint64   = 1000
	emptyVideoState                        = workerVideoState{}
	simpleVideoStatus                      = VideoStatus{
		Filename: &testWorkerVideo,
		Position: &testWorkerPosition,
		Paused:   notReady,
		Speed:    defaultWorkerSpeed,
	}
	simplePlaylist  = []string{testWorkerVideo, testWorkerVideo2}
	simpleRoomState = RoomState{
		playlist: simplePlaylist,
		video:    &testWorkerVideo,
		duration: Duration{0},
		position: &testWorkerPositionMillisecond,
		lastSeek: Duration{0},
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

	require.NotNil(t, worker.state.uuid)
	require.Nil(t, worker.room)
	require.NotNil(t, worker.state.loggedIn)
	require.Empty(t, worker.state.stopChan)

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
}

func TestWorkerStartClose(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWebsocket := setUpMockWebsocket(ctrl, nil)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	worker := &Worker{
		websocket:   mockWebsocket,
		roomHandler: mockRoomHandler,
		state: workerState{
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency: workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)
}

func setUpMockWebsocket(ctrl *gomock.Controller, readMessage []byte) *MockWebsocketReaderWriter {
	mockWebsocket := NewMockWebsocketReaderWriter(ctrl)
	mockWebsocket.EXPECT().
		ReadMessage().
		DoAndReturn(func() ([]byte, error) {
			time.Sleep(messageDelay)
			return readMessage, nil
		}).
		MinTimes(1)

	mockWebsocket.EXPECT().
		WriteMessage(gomock.Any()).
		DoAndReturn(func(payload []byte) error {
			return nil
		}).
		AnyTimes()

	mockWebsocket.EXPECT().
		Close().
		Return(nil)

	return mockWebsocket
}

func testStartCloseWorker(t *testing.T, worker ClientWorker) {
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
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	worker := &Worker{
		websocket:   mockWebsocket,
		room:        mockRoom,
		roomHandler: mockRoomHandler,
		state: workerState{
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency: workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)
}

func setUpMockRoom(
	ctrl *gomock.Controller, roomState RoomState,
	useAllUsersReady bool, allUsersReady bool,
	shouldBeClosed bool, useRoomState bool) *MockRoomStateHandler {
	mockRoom := NewMockRoomStateHandler(ctrl)

	if useRoomState {
		mockRoom.EXPECT().
			RoomState().
			Return(roomState).
			MinTimes(0)
	}

	if useAllUsersReady {
		mockRoom.EXPECT().
			Ready().
			Return(allUsersReady).
			MinTimes(1)
	}

	mockRoom.EXPECT().
		SetPaused(gomock.Any()).
		MinTimes(1)

	mockRoom.EXPECT().
		IsEmpty().
		Return(true).
		MinTimes(1)

	mockRoom.EXPECT().
		DeleteWorker(gomock.Any())

	mockRoom.EXPECT().
		ShouldBeClosed().
		Return(shouldBeClosed)

	return mockRoom
}

func setUpMockRoomHandler(ctrl *gomock.Controller, minBroadcasts int, expectedDeleteRoom int) *MockServerStateHandler {
	mockRoomHandler := NewMockServerStateHandler(ctrl)
	mockRoomHandler.EXPECT().
		BroadcastStatusList().
		MinTimes(minBroadcasts)

	mockRoomHandler.EXPECT().
		DeleteRoom(gomock.Any()).
		Times(expectedDeleteRoom)

	return mockRoomHandler
}

func TestCloseBeforeStart(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWebsocket := setUpMockWebsocket(ctrl, nil)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	worker := &Worker{
		websocket:   mockWebsocket,
		roomHandler: mockRoomHandler,
		state: workerState{
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency: workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	worker.Close()
	testStartCloseWorker(t, worker)
}

func TestShutdown(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWebsocket := setUpMockWebsocket(ctrl, nil)
	worker := &Worker{
		websocket: mockWebsocket,
		state: workerState{
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		latency: workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
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
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)

	worker := &Worker{
		websocket:   mockWebsocket,
		roomHandler: mockRoomHandler,
		state: workerState{
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		latency: workerLatency{timestamps: timestamps},
	}
	testStartCloseWorker(t, worker)
	require.NotEmpty(t, worker.latency.roundTripTime)
}

func TestHandleStatus(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	status := []byte(fmt.Sprintf(`{"ready":%t,"username":"%s","type":"status"}`, notReady, username))
	playingRoomState := simpleRoomState
	playingRoomState.paused = true
	mockRoom := setUpMockRoom(ctrl, playingRoomState, true, false, false, true)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 2, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, status)

	mockRoom.EXPECT().
		SetWorkerStatus(
			gomock.Eq(defaultUUID),
			gomock.Eq(Status{Ready: notReady, Username: username})).
		MinTimes(1)

	mockRoomHandler.EXPECT().
		RenameUserIfUnavailable(gomock.Eq(username)).
		Return(username).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		userStatus:   Status{Ready: true, Username: ""},
		latencyMutex: sync.RWMutex{},
	}
	testStartCloseWorker(t, worker)
	require.NotEmpty(t, worker.userStatus)
	require.Equal(t, notReady, worker.userStatus.Ready)
	require.Equal(t, username, worker.userStatus.Username)
}

// TODO test all users ready case
func TestHandleVideoStatusEqualStates(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(
		fmt.Sprintf(
			`{"filename":"%s","duration":%d,"position":%d,"paused":%t,"speed":%g,"cache":%d,"username":"%s","type":"videoStatus"}`,
			testWorkerVideo, testWorkerDurationMillis, testWorkerPositionMillis, notPaused, defaultSpeed, testWorkerCacheMillis, username,
		),
	)
	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus, true)
	testStartCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo, *worker.videoState.video)
	require.Equal(t, testWorkerPositionMillisecond, *worker.videoState.position)
	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func setUpWorkerForVideoStatus(t *testing.T, ctrl *gomock.Controller, videoStatus []byte, useSyncing bool) *Worker {
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, true)

	pos := testWorkerPosition.MultInt(int(time.Millisecond))
	if useSyncing {
		mockRoom.EXPECT().
			SlowestEstimatedClientPosition().
			Return(&pos).
			MinTimes(1)

		mockRoom.EXPECT().
			SetPosition(gomock.Eq(pos)).
			MinTimes(1)

		mockRoom.EXPECT().
			SetDuration(gomock.Eq(testWorkerDuration)).
			MinTimes(1)

		mockRoom.EXPECT().
			HandleCache(gomock.Eq(&testWorkerCache), gomock.Eq(workerUUIDS[0]), gomock.Eq(username)).
			MinTimes(1)
	}

	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, videoStatus)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      workerUUIDS[0],
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState: workerVideoState{},
		userStatus: Status{Ready: true, Username: username},
		latency:    workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	return worker
}

func TestHandleVideoStatusIncorrectFilename(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(
		fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
			testWorkerVideo2, testWorkerPosition.Uint64(), notPaused, defaultSpeed, username,
		),
	)
	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus, false)
	testStartCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo2, *worker.videoState.video)
	require.Equal(t, testWorkerPositionMillisecond, *worker.videoState.position)
	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func TestHandleVideoStatusIncorrectSpeed(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(
		fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
			testWorkerVideo, testWorkerPosition.Uint64(), notPaused, 2.0, username,
		),
	)
	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus, false)
	testStartCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo, *worker.videoState.video)
	require.Equal(t, testWorkerPositionMillisecond, *worker.videoState.position)
	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, 2.0, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func TestHandleVideoStatusIncorrectPaused(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	videoStatus := []byte(
		fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
			testWorkerVideo, testWorkerPosition.Uint64(), true, defaultSpeed, username,
		),
	)
	worker := setUpWorkerForVideoStatus(t, ctrl, videoStatus, false)
	testStartCloseWorker(t, worker)

	require.Equal(t, testWorkerVideo, *worker.videoState.video)
	require.Equal(t, testWorkerPositionMillisecond, *worker.videoState.position)
	require.Equal(t, true, worker.videoState.paused)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
	require.NotEmpty(t, worker.videoState.timestamp)
}

func TestHandleStart(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	status := []byte(fmt.Sprintf(`{"username":"%s","type":"start"}`, username))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, status)
	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(status), gomock.Eq(defaultUUID)).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:   simpleVideoState,
		userStatus:   Status{Username: username},
		latencyMutex: sync.RWMutex{},
	}
	testStartCloseWorker(t, worker)
	require.Equal(t, false, worker.videoState.paused)
}

func TestHandleSeek(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	seek := []byte(
		fmt.Sprintf(`{"filename":"%s","position":%d,"desync":%t,"username":"","type":"seek"}`,
			testWorkerVideo, testWorkerPosition.Uint64(), false,
		),
	)
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, true)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, seek)
	var boolNil *bool
	boolNil = nil
	var floatNil *float64
	floatNil = nil
	mockRoom.EXPECT().
		SetPlaylistState(gomock.Any(), gomock.Eq(testWorkerPositionMillisecond),
			gomock.Eq(boolNil), gomock.Eq(&testWorkerPositionMillisecond), gomock.Eq(floatNil)).
		Do(func(filename *string, position Duration, paused *bool, lastSeek *Duration, speed *float64) {
			require.Equal(t, testWorkerVideo, *filename)
		}).
		MinTimes(1)

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(seek), gomock.Eq(defaultUUID)).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState:   workerVideoState{speed: 1},
		latencyMutex: sync.RWMutex{},
	}
	testStartCloseWorker(t, worker)

	require.Equal(t, notPaused, worker.videoState.paused)
	require.Equal(t, testWorkerPositionMillisecond, *worker.videoState.position)
	require.Equal(t, float64(1), worker.videoState.speed)
	require.Equal(t, testWorkerVideo, *worker.videoState.video)
}

func TestHandleSelect(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	sel := []byte(fmt.Sprintf(`{"filename":"%s","position":%d,"username":"","type":"select"}`, testWorkerVideo, testWorkerPosition.Uint64()))
	playingRoomState := simpleRoomState
	playingRoomState.paused = true
	mockRoom := setUpMockRoom(ctrl, playingRoomState, true, false, false, true)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, sel)
	truePointer := true
	var floatPointer *float64
	floatPointer = nil
	mockRoom.EXPECT().
		SetPlaylistState(gomock.Any(), gomock.Eq(testWorkerPositionMillisecond), gomock.Eq(&truePointer), gomock.Eq(&testWorkerPositionMillisecond), gomock.Eq(floatPointer)).
		Do(func(filename *string, position Duration, paused *bool, lastSeek *Duration, speed *float64) {
			require.Equal(t, testWorkerVideo, *filename)
		}).
		MinTimes(1)

	mockRoom.EXPECT().
		BroadcastAll(gomock.Eq(sel)).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState: workerVideoState{speed: 1},
		latency:    workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)

	require.Equal(t, paused, worker.videoState.paused)
	require.Equal(t, testWorkerPositionMillisecond, *worker.videoState.position)
	require.Equal(t, float64(1), worker.videoState.speed)
	require.Equal(t, testWorkerVideo, *worker.videoState.video)
}

func TestHandleUserMessage(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	userMessage := []byte(
		fmt.Sprintf(`{"message":"%s","username":"%s","type":"userMessage"}`,
			testWorkerVideo, username,
		),
	)
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, userMessage)
	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(userMessage), gomock.Eq(defaultUUID)).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		userStatus: Status{Username: username},
		latency:    workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)
}

// TODO test playlist change
func TestHandlePlaylist(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	jsonPlaylist, err := json.Marshal(simplePlaylist)
	require.NoError(t, err)
	playlist := []byte(fmt.Sprintf(`{"playlist":%s,"username":"","type":"playlist"}`, string(jsonPlaylist)))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, true)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, playlist)
	mockRoom.EXPECT().
		BroadcastAll(gomock.Eq(playlist)).
		MinTimes(1)

	mockRoom.EXPECT().
		SetPlaylist(gomock.Eq(simplePlaylist)).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		latencyMutex: sync.RWMutex{},
	}
	testStartCloseWorker(t, worker)
}

func TestHandlePause(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	pause := []byte(fmt.Sprintf(`{"username":"","type":"pause"}`))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, pause)
	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(pause), gomock.Eq(defaultUUID)).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState: workerVideoState{},
		latency:    workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)
	require.True(t, worker.videoState.paused)
}

func TestHandleFailedJoin(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	join := []byte(
		fmt.Sprintf(`{"password":"%s","room":"%s","username":"%s","type":"join"}`,
			"testPassword", "testRoom", username,
		),
	)
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, join)
	mockRoomHandler.EXPECT().
		IsPasswordCorrect(gomock.Eq("testPassword")).
		Return(false).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		videoState: workerVideoState{},
		latency:    workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)
	require.False(t, worker.state.loggedIn)
}

func TestHandleJoin(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	join := []byte(
		fmt.Sprintf(`{"password":"%s","room":"%s","username":"%s","type":"join"}`,
			"testPassword", "testRoom", username,
		),
	)
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, true)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, join)
	mockRoom.EXPECT().
		DeleteWorker(gomock.Eq(defaultUUID)).
		MinTimes(1)

	mockRoom.EXPECT().
		ShouldBeClosed().
		Return(true).
		MinTimes(1)

	mockRoom.EXPECT().
		Close().
		MinTimes(1)

	mockRoom.EXPECT().
		SetWorkerStatus(gomock.Eq(defaultUUID), gomock.Eq(Status{Ready: false, Username: username})).
		MinTimes(1)

	mockRoomHandler.EXPECT().
		CreateOrFindRoom(gomock.Eq("testRoom")).
		Return(mockRoom, nil).
		MinTimes(1)

	mockRoomHandler.EXPECT().
		IsPasswordCorrect(gomock.Eq("testPassword")).
		Return(true).
		MinTimes(1)

	mockRoomHandler.EXPECT().
		AppendRoom(gomock.Eq(mockRoom)).
		MinTimes(1)

	mockRoomHandler.EXPECT().
		DeleteRoom(gomock.Eq(mockRoom)).
		MinTimes(1)

	mockRoomHandler.EXPECT().
		RenameUserIfUnavailable(gomock.Eq(username)).
		Return(username).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
		},
		videoState: workerVideoState{},
		latency:    workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}

	mockRoom.EXPECT().
		AppendWorker(worker).
		MinTimes(1)

	testStartCloseWorker(t, worker)
	require.True(t, worker.state.loggedIn)
}

func TestHandlePlaybackSpeed(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	playbackSpeed := []byte(fmt.Sprintf(`{"speed":%g,"username":"","type":"playbackSpeed"}`, defaultSpeed))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, playbackSpeed)

	mockRoom.EXPECT().
		BroadcastExcept(gomock.Eq(playbackSpeed), gomock.Eq(defaultUUID)).
		MinTimes(1)

	mockRoom.EXPECT().
		SetSpeed(gomock.Eq(defaultSpeed)).
		MinTimes(1)

	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		videoState: workerVideoState{},
		latency:    workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)
	require.Equal(t, defaultSpeed, worker.videoState.speed)
}

func TestHandleUnknown(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	unknown := []byte(fmt.Sprintf(`{"someObject":"someText,"type":"someType"}`))
	mockRoom := setUpMockRoom(ctrl, simpleRoomState, false, false, false, false)
	mockRoomHandler := setUpMockRoomHandler(ctrl, 1, 0)
	mockWebsocket := setUpMockWebsocket(ctrl, unknown)
	worker := &Worker{
		roomHandler: mockRoomHandler,
		websocket:   mockWebsocket,
		room:        mockRoom,
		state: workerState{
			uuid:      defaultUUID,
			stopChan:  make(chan int),
			taskChan:  make(chan Task, 10),
			writeChan: make(chan []byte, 10),
			loggedIn:  true,
		},
		latency: workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
	}
	testStartCloseWorker(t, worker)
}

func TestSendPing(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	worker := &Worker{
		latency: workerLatency{timestamps: make(map[uuid.UUID]time.Time)},
		state:   workerState{writeChan: make(chan []byte, 2)},
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
		latency: workerLatency{timestamps: map[uuid.UUID]time.Time{
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
		latency: workerLatency{timestamps: map[uuid.UUID]time.Time{
			workerUUIDS[0]: farDate,
			workerUUIDS[1]: farDate,
		}},
	}
	worker.deletePings()
	require.Len(t, worker.latency.timestamps, 0)
}

func testDeletePingsNone(t *testing.T) {
	worker := &Worker{
		latency: workerLatency{timestamps: map[uuid.UUID]time.Time{
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
	require.Equal(t, simpleStatus, worker.userStatus)

	worker.SetUserStatus(emptyStatus)
	require.Equal(t, emptyStatus, worker.userStatus)
}

func TestVideoState(t *testing.T) {
	worker := Worker{
		videoState: simpleVideoState,
	}
	videoState := worker.VideoState()
	require.Equal(t, simpleVideoState, videoState)
}

func TestSendSeek(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockRoom := NewMockRoomStateHandler(ctrl)
	mockRoom.EXPECT().
		RoomState().
		Return(simpleRoomState).
		MinTimes(1)

	worker := &Worker{
		room: mockRoom,
		state: workerState{
			writeChan: make(chan []byte, 10),
		},
		videoState: workerVideoState{},
		latency:    workerLatency{},
	}
	worker.sendSeek(true)
	message := <-worker.state.writeChan
	expectedMessage := []byte(
		fmt.Sprintf(`{"filename":"%s","position":%d,"desync":%t,"username":"","type":"seek"}`,
			*simpleRoomState.video, testWorkerPosition.Uint64(), true,
		),
	)
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
		Return(simpleRoomState).
		MinTimes(1)

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
		videoState: workerVideoState{position: nil, timestamp: timestamp, paused: false, speed: defaultSpeed},
	}
	estimatedPosition := worker.EstimatePosition()
	require.Nil(t, estimatedPosition)

	edgePosition := Duration{0}
	worker.videoState.position = &edgePosition
	estimatedPosition = worker.EstimatePosition()
	expectedPosition := TimeSince(timestamp)
	require.NotZero(t, estimatedPosition.Uint64())
	require.GreaterOrEqual(t, expectedPosition.Uint64(), estimatedPosition.Uint64())

	properPosition := Duration{10000}
	worker.videoState.position = &properPosition
	estimatedPosition = worker.EstimatePosition()
	expectedPosition = expectedPosition.Add(properPosition)
	require.NotZero(t, estimatedPosition.Uint64())
	require.GreaterOrEqual(t, estimatedPosition.Uint64(), expectedPosition.Uint64())

	worker.videoState.paused = true
	estimatedPosition = worker.EstimatePosition()
	expectedPosition = properPosition
	require.Equal(t, expectedPosition.Uint64(), estimatedPosition.Uint64())
}
