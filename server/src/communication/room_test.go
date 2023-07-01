package communication

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/golang/mock/gomock"
	"github.com/google/uuid"
	"github.com/stretchr/testify/require"
)

var (
	playlist        []string = []string{"testVideo1", "testVideo2"}
	defaultPlaylist []string = make([]string, 0)
	video           *string
	video2          *string
	defaultPosition *uint64
	highPosition    *uint64
	defaultLastSeek uint64      = 0
	highLastSeek    uint64      = 10000
	defaultPaused   bool        = false
	notPaused       bool        = true
	defaultSpeed    float64     = 1.0
	highSpeed       float64     = 2.0
	testPayload     []byte      = []byte("testMessage")
	uuids           []uuid.UUID = []uuid.UUID{
		uuid.Must(uuid.Parse("123e4567-e89b-12d3-a456-426655440000")),
		uuid.Must(uuid.Parse("123e4567-e89b-12d3-a456-426655440001")),
		uuid.Must(uuid.Parse("123e4567-e89b-12d3-a456-426655440002")),
	}
)

const (
	roomName            string = "testRoom"
	failedRoomName      string = "test/room"
	dbPath              string = "."
	failedDBPath        string = "some$ / malformed& \\ Path"
	dbUpdateInterval    uint64 = 1
	dbWaitTimeout       uint64 = 1
	failedDBWaitTimeout uint64 = 0
	persistent          bool   = false
)

func init() {
	vid := "testVideo"
	video = &vid
	vid2 := "testVideo2"
	video2 = &vid2

	pos := uint64(0)
	defaultPosition = &pos
	pos2 := uint64(1000000)
	highPosition = &pos2
}

func TestNewRoom(t *testing.T) {
	room, err := NewRoom(roomName, dbPath, dbUpdateInterval, dbWaitTimeout, persistent)
	testCorrectState(t, room, err)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func testCorrectState(t *testing.T, room *Room, err error) {
	require.NoError(t, err)
	require.NotNil(t, room)
	require.Equal(t, roomName, room.name)
	require.DirExists(t, dbPath)
	require.FileExists(t, filepath.Join(dbPath, roomName+".db"))
	require.Empty(t, room.workers)
	require.NotNil(t, room.workersMutex)
	require.NotNil(t, room.state)
	require.Empty(t, room.state.playlist)
	require.Empty(t, room.state.video)
	require.Empty(t, room.state.position)
	require.Equal(t, uint64(0), room.state.lastSeek)
	require.Equal(t, true, room.state.paused)
	require.Equal(t, 1.0, room.state.speed)
	require.NotNil(t, room.stateMutex)
	require.NotNil(t, room.db)
	require.Equal(t, room.dbUpdateInterval, time.Duration(uint64(time.Second)*dbUpdateInterval))
	require.Empty(t, room.dbChannel)
	require.Equal(t, persistent, room.persistent)
}

func TestFailedNewRoom(t *testing.T) {
	room, err := NewRoom(roomName, failedDBPath, dbUpdateInterval, dbWaitTimeout, persistent)
	require.Error(t, err)
	require.Nil(t, room)

	room, err = NewRoom(roomName, dbPath, dbUpdateInterval, failedDBWaitTimeout, persistent)
	require.Error(t, err)
	require.Nil(t, room)

	room, err = NewRoom(failedRoomName, dbPath, dbUpdateInterval, dbWaitTimeout, persistent)
	require.Error(t, err)
	require.Nil(t, room)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
		os.Remove(filepath.Join(failedDBPath, roomName+".db"))
		os.Remove(filepath.Join(dbPath, failedRoomName+".db"))
	})
}

func TestStartClose(t *testing.T) {
	room, _ := NewRoom(roomName, dbPath, dbUpdateInterval, dbWaitTimeout, persistent)
	go startRoom(t, room)
	time.Sleep(2 * time.Second)
	room.Close()

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func startRoom(t *testing.T, room *Room) {
	room.Start()
	t.Failed()
}

func TestWritePlaylist(t *testing.T) {
	room, _ := NewRoom(roomName, dbPath, dbUpdateInterval, dbWaitTimeout, persistent)
	state := &RoomState{playlist: playlist, video: video, position: defaultPosition, lastSeek: defaultLastSeek, paused: defaultPaused, speed: defaultSpeed}
	testCorrectWritePlaylist(t, room, state)

	state = &RoomState{playlist: defaultPlaylist, video: video2, position: highPosition, lastSeek: defaultLastSeek, paused: defaultPaused, speed: defaultSpeed}
	testCorrectWritePlaylist(t, room, state)

	testFailedWritePlaylist(t, room)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func testCorrectWritePlaylist(t *testing.T, room *Room, state *RoomState) {
	room.state = state
	err := room.writePlaylist()
	require.NoError(t, err)
	testCorrectDBState(t, room)
}

func testFailedWritePlaylist(t *testing.T, room *Room) {
	room.Close()
	err := room.writePlaylist()
	require.Error(t, err)
}

func testCorrectDBState(t *testing.T, room *Room) {
	playlistBytes, err := room.playlist()
	require.NoError(t, err)
	var playlist []string
	json.Unmarshal(playlistBytes, &playlist)
	require.NoError(t, err)
	require.Equal(t, playlist, room.state.playlist)

	positionBytes, err := room.position()
	require.NoError(t, err)
	var position uint64
	binary.Read(bytes.NewBuffer(positionBytes[:]), binary.LittleEndian, &position)
	require.NoError(t, err)
	require.Equal(t, position, *room.state.position)

	videoBytes, err := room.video()
	require.NoError(t, err)
	video := string(videoBytes)
	require.NoError(t, err)
	require.Equal(t, video, *room.state.video)
}

func TestRoomClose(t *testing.T) {
	room, _ := NewRoom(roomName, dbPath, dbUpdateInterval, dbWaitTimeout, persistent)
	testCorrectClose(t, room)
	testFailedClose(t, room)
	testClosedDB(t, room)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func testCorrectClose(t *testing.T, room *Room) {
	room.state = &RoomState{playlist: playlist, video: video, position: defaultPosition, lastSeek: defaultLastSeek, paused: defaultPaused, speed: defaultSpeed}
	room.writePlaylist()
	err := room.Close()
	require.NoError(t, err)
	require.NoFileExists(t, filepath.Join(dbPath, roomName+".db"))
}

func testFailedClose(t *testing.T, room *Room) {
	defer func() {
		if r := recover(); r == nil {
			t.Errorf("Second Close did not panic")
		}
	}()

	room.Close()
}

func testClosedDB(t *testing.T, room *Room) {
	_, err := room.playlist()
	require.Error(t, err)

	_, err = room.video()
	require.Error(t, err)

	_, err = room.position()
	require.Error(t, err)
}

func TestRoomStart(t *testing.T) {
	room, _ := NewRoom(roomName, dbPath, dbUpdateInterval, dbWaitTimeout, persistent)
	room.state = &RoomState{playlist: playlist, video: video, position: defaultPosition, lastSeek: defaultLastSeek, paused: defaultPaused, speed: defaultSpeed}
	go room.Start()
	time.Sleep(2 * time.Second)
	room.closeChan()
	testCorrectDBState(t, room)

	room.state = &RoomState{playlist: defaultPlaylist, video: video2, position: highPosition, lastSeek: defaultLastSeek, paused: defaultPaused, speed: defaultSpeed}
	go room.Start()
	time.Sleep(2 * time.Second)
	room.closeChan()
	testCorrectDBState(t, room)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func TestAppendWorker(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	room := &Room{
		workers:      []ClientWorker{},
		workersMutex: &sync.RWMutex{},
	}
	room.AppendWorker(NewMockClientWorker(ctrl))
	require.Len(t, room.workers, 1)

	room.AppendWorker(NewMockClientWorker(ctrl))
	require.Len(t, room.workers, 2)

	room.workers = []ClientWorker{NewMockClientWorker(ctrl), NewMockClientWorker(ctrl),
		NewMockClientWorker(ctrl), NewMockClientWorker(ctrl), NewMockClientWorker(ctrl)}
	require.Len(t, room.workers, 5)

	room.AppendWorker(NewMockClientWorker(ctrl))
	require.Len(t, room.workers, 6)
}

func TestDeleteWorker(t *testing.T) {
	testEmptyDeleteWorker(t)
	testCorrectDeleteWorker(t)
}

func testEmptyDeleteWorker(t *testing.T) {
	room := &Room{
		workers:      []ClientWorker{},
		workersMutex: &sync.RWMutex{},
	}
	uuid := getUID(t)
	require.Len(t, room.workers, 0)
	room.DeleteWorker(uuid)
	require.Len(t, room.workers, 0)
}

func testCorrectDeleteWorker(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	room := &Room{
		workers: []ClientWorker{getMockClientWithUUID(t, ctrl, uuids[0], 1, 0),
			getMockClientWithUUID(t, ctrl, uuids[1], 3, 0),
			getMockClientWithUUID(t, ctrl, uuids[2], 3, 0)},
		workersMutex: &sync.RWMutex{},
	}
	require.Len(t, room.workers, 3)
	room.DeleteWorker(uuids[0])
	require.Len(t, room.workers, 2)

	room.DeleteWorker(uuids[2])
	require.Len(t, room.workers, 1)

	room.DeleteWorker(uuids[0])
	require.Len(t, room.workers, 1)

	room.DeleteWorker(uuids[1])
	require.Len(t, room.workers, 0)
}

func getUID(t *testing.T) uuid.UUID {
	uuid, err := uuid.NewUUID()
	require.NoError(t, err)
	return uuid
}

func getAtomicUUID(t *testing.T) *atomic.Pointer[uuid.UUID] {
	atomicUUID := atomic.Pointer[uuid.UUID]{}
	uuid := getUID(t)
	atomicUUID.Store(&uuid)
	return &atomicUUID
}

func TestDeleteAppendWorker(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	room := &Room{
		workers:      []ClientWorker{},
		workersMutex: &sync.RWMutex{},
	}

	mockWorker := getMockClientWithUUID(t, ctrl, uuids[0], 2, 0)
	room.AppendWorker(mockWorker)
	require.Len(t, room.workers, 1)
	room.DeleteWorker(uuids[0])
	require.Len(t, room.workers, 0)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func TestBroadcastExcept(t *testing.T) {
	testBroadcastExceptMultipleWorkers(t)
	testBroadcastExceptOneWorker(t)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func testBroadcastExceptMultipleWorkers(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWorker1 := getMockClientWithUUID(t, ctrl, uuids[0], 1, 0)
	mockWorker2 := getMockClientWithUUID(t, ctrl, uuids[1], 1, 1)

	room := &Room{
		workers:      []ClientWorker{mockWorker1, mockWorker2},
		workersMutex: &sync.RWMutex{},
	}
	room.BroadcastExcept(testPayload, uuids[0])
}

func testBroadcastExceptOneWorker(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()
	mockWorker1 := getMockClientWithUUID(t, ctrl, uuids[0], 1, 0)

	room := &Room{
		workers:      []ClientWorker{mockWorker1},
		workersMutex: &sync.RWMutex{},
	}
	room.BroadcastExcept(testPayload, uuids[0])

	mockWorker1 = getMockClientWithUUID(t, ctrl, uuids[0], 1, 1)
	room.workers = []ClientWorker{mockWorker1}

	room.BroadcastExcept(testPayload, uuids[1])
}

func getMockClientWithUUID(t *testing.T, ctrl *gomock.Controller, newUUID uuid.UUID, maxTimesUUID int, maxTimesSendMessage int) ClientWorker {
	m := NewMockClientWorker(ctrl)

	m.EXPECT().
		UUID().
		Return(&newUUID).
		MaxTimes(maxTimesUUID)

	m.EXPECT().
		SendMessage(gomock.Eq(testPayload)).
		Do(func(actualPayload []byte) {
			require.Equal(t, testPayload, actualPayload)
		}).
		MaxTimes(maxTimesSendMessage)

	return m
}

func TestBroadcastAll(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWorker1 := getMockClient(t, ctrl)
	mockWorker2 := getMockClient(t, ctrl)
	room := &Room{
		workers:      []ClientWorker{mockWorker1, mockWorker2},
		workersMutex: &sync.RWMutex{},
	}
	room.BroadcastAll(testPayload)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func getMockClient(t *testing.T, ctrl *gomock.Controller) ClientWorker {
	m := NewMockClientWorker(ctrl)

	m.EXPECT().
		SendMessage(gomock.Eq(testPayload)).
		Do(func(actualPayload []byte) {
			require.Equal(t, testPayload, actualPayload)
		}).
		Times(1)

	return m
}

func TestAllUsersReady(t *testing.T) {
	testAllReady(t)
	testNotAllReady(t)
}

func testAllReady(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	testAllReadyMultipleUsers(t, ctrl)
	testAllReadyOneUser(t, ctrl)
	testAllReadyNoUsers(t)
}

func testAllReadyMultipleUsers(t *testing.T, ctrl *gomock.Controller) {
	mockWorker1 := getMockClientWithUserStatus(t, ctrl, true)
	mockWorker2 := getMockClientWithUserStatus(t, ctrl, true)
	room := &Room{
		workers:      []ClientWorker{mockWorker1, mockWorker2},
		workersMutex: &sync.RWMutex{},
	}
	allReady := room.AllUsersReady()
	require.Equal(t, true, allReady)
}

func testAllReadyOneUser(t *testing.T, ctrl *gomock.Controller) {
	mockWorker1 := getMockClientWithUserStatus(t, ctrl, true)
	room := &Room{
		workers:      []ClientWorker{mockWorker1},
		workersMutex: &sync.RWMutex{},
	}
	allReady := room.AllUsersReady()
	require.Equal(t, true, allReady)
}

func testAllReadyNoUsers(t *testing.T) {
	room := &Room{
		workers:      []ClientWorker{},
		workersMutex: &sync.RWMutex{},
	}
	allReady := room.AllUsersReady()
	require.Equal(t, true, allReady)
}

func testNotAllReady(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	testNotAllReadyMultipleUsers(t, ctrl)
	testNotAllOneUser(t, ctrl)
}

func testNotAllReadyMultipleUsers(t *testing.T, ctrl *gomock.Controller) {
	mockWorker1 := getMockClientWithUserStatus(t, ctrl, false)
	mockWorker2 := getMockClientWithUserStatus(t, ctrl, true)
	room := &Room{
		workers:      []ClientWorker{mockWorker1, mockWorker2},
		workersMutex: &sync.RWMutex{},
	}
	allReady := room.AllUsersReady()
	require.Equal(t, false, allReady)
}

func testNotAllOneUser(t *testing.T, ctrl *gomock.Controller) {
	mockWorker1 := getMockClientWithUserStatus(t, ctrl, false)
	room := &Room{
		workers:      []ClientWorker{mockWorker1},
		workersMutex: &sync.RWMutex{},
	}
	allReady := room.AllUsersReady()
	require.Equal(t, false, allReady)
}

func getMockClientWithUserStatus(t *testing.T, ctrl *gomock.Controller, ready bool) ClientWorker {
	m := NewMockClientWorker(ctrl)

	m.EXPECT().
		UserStatus().
		Return(&Status{Ready: ready}).
		Times(1)

	return m
}

func TestFastestClientPosition(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	testFastestClientPositionMultipleUsers(t, ctrl)
	testFastestClientPositionOneUser(t, ctrl)
	testFastestClientPositionOneUser(t, ctrl)
}

func testFastestClientPositionMultipleUsers(t *testing.T, ctrl *gomock.Controller) {
	mockWorker1 := getMockClientWithEstimatePosition(t, ctrl, defaultPosition)
	mockWorker2 := getMockClientWithEstimatePosition(t, ctrl, highPosition)
	room := &Room{
		workers:      []ClientWorker{mockWorker1, mockWorker2},
		workersMutex: &sync.RWMutex{},
	}
	fastestPosition := room.FastestClientPosition()
	require.Equal(t, *highPosition, fastestPosition)

	mockWorker1 = getMockClientWithEstimatePosition(t, ctrl, defaultPosition)
	mockWorker2 = getMockClientWithEstimatePosition(t, ctrl, nil)
	room.workers = []ClientWorker{mockWorker1, mockWorker2}
	fastestPosition = room.FastestClientPosition()
	require.Equal(t, *defaultPosition, fastestPosition)
}

func testFastestClientPositionOneUser(t *testing.T, ctrl *gomock.Controller) {
	mockWorker1 := getMockClientWithEstimatePosition(t, ctrl, highPosition)
	room := &Room{
		workers:      []ClientWorker{mockWorker1},
		workersMutex: &sync.RWMutex{},
	}
	fastestPosition := room.FastestClientPosition()
	require.Equal(t, *highPosition, fastestPosition)

	mockWorker1 = getMockClientWithEstimatePosition(t, ctrl, nil)
	room.workers = []ClientWorker{mockWorker1}
	fastestPosition = room.FastestClientPosition()
	require.Equal(t, *defaultPosition, fastestPosition)
}

func testFastestClientPositionNoUsers(t *testing.T, ctrl *gomock.Controller) {
	room := &Room{
		workers:      []ClientWorker{},
		workersMutex: &sync.RWMutex{},
	}
	fastestPosition := room.FastestClientPosition()
	require.Equal(t, *defaultPosition, fastestPosition)
}

func getMockClientWithEstimatePosition(t *testing.T, ctrl *gomock.Controller, position *uint64) ClientWorker {
	m := NewMockClientWorker(ctrl)

	m.EXPECT().
		EstimatePosition().
		Return(position).
		MaxTimes(1)

	return m
}

func TestSetPlaylistState(t *testing.T) {
	room := &Room{
		state:      &RoomState{},
		stateMutex: &sync.RWMutex{},
	}
	room.SetPlaylistState(video, *defaultPosition, defaultPaused, defaultLastSeek)
	require.Equal(t, video, room.state.video)
	require.Equal(t, *defaultPosition, *room.state.position)
	require.Equal(t, defaultLastSeek, room.state.lastSeek)
	require.Equal(t, defaultPaused, room.state.paused)

	room.SetPlaylistState(nil, *highPosition, notPaused, highLastSeek)
	require.Empty(t, room.state.video)
	require.Equal(t, *highPosition, *room.state.position)
	require.Equal(t, highLastSeek, room.state.lastSeek)
	require.Equal(t, notPaused, room.state.paused)
}

func TestSetPosition(t *testing.T) {
	room := &Room{
		state:      &RoomState{},
		stateMutex: &sync.RWMutex{},
	}
	room.state.lastSeek = defaultLastSeek
	room.SetPosition(*defaultPosition)
	require.Equal(t, *defaultPosition, *room.state.position)

	room.SetPosition(*highPosition)
	require.Equal(t, *highPosition, *room.state.position)

	room.state.lastSeek = highLastSeek
	room.SetPosition(*defaultPosition)
	require.Equal(t, highLastSeek, *room.state.position)

	room.state.lastSeek = highLastSeek
	room.SetPosition(*defaultPosition)
	require.Equal(t, highLastSeek, *room.state.position)
}

func TestSetLastSeek(t *testing.T) {
	room := &Room{
		stateMutex: &sync.RWMutex{},
		state:      &RoomState{},
	}

	room.SetLastSeek(highLastSeek)
	require.Equal(t, highLastSeek, room.state.lastSeek)

	room.SetLastSeek(defaultLastSeek)
	require.Equal(t, defaultLastSeek, room.state.lastSeek)
}

func TestSetSpeed(t *testing.T) {
	room := &Room{
		stateMutex: &sync.RWMutex{},
		state:      &RoomState{},
	}

	room.SetSpeed(highSpeed)
	require.Equal(t, highSpeed, room.state.speed)

	room.SetSpeed(defaultSpeed)
	require.Equal(t, defaultSpeed, room.state.speed)
}

func TestSetPaused(t *testing.T) {
	room := &Room{
		stateMutex: &sync.RWMutex{},
		state:      &RoomState{},
	}

	room.SetPaused(notPaused)
	require.Equal(t, notPaused, room.state.paused)

	room.SetPaused(defaultPaused)
	require.Equal(t, defaultPaused, room.state.paused)
}

func TestSetStateFromDB(t *testing.T) {
	room, _ := NewRoom(roomName, dbPath, dbUpdateInterval, dbWaitTimeout, persistent)
	room.state = &RoomState{
		playlist: playlist,
		video:    video,
		position: defaultPosition,
	}
	err := room.writePlaylist()
	require.NoError(t, err)

	room.state = &RoomState{}
	room.setStateFromDB()
	require.Equal(t, playlist, room.state.playlist)
	require.Equal(t, video, room.state.video)
	require.Equal(t, *defaultPosition, *room.state.position)

	t.Cleanup(func() {
		os.Remove(filepath.Join(dbPath, roomName+".db"))
	})
}

func TestName(t *testing.T) {
	room := Room{name: "test"}

	name := room.Name()
	require.Equal(t, "test", name)
}

func TestWorkerStatus(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	mockWorker1 := NewMockClientWorker(ctrl)
	mockWorker2 := NewMockClientWorker(ctrl)
	expectedStatus1 := Status{Username: testUsername, Ready: defaultPaused}
	expectedStatus2 := Status{Username: testUsername2, Ready: notPaused}

	gomock.InOrder(
		mockWorker1.EXPECT().
			UserStatus().
			Return(&expectedStatus1).
			Times(1),
		mockWorker2.EXPECT().
			UserStatus().
			Return(&expectedStatus2).
			Times(1),
	)

	room := &Room{
		workersMutex: &sync.RWMutex{},
		workers:      []ClientWorker{mockWorker1, mockWorker2},
	}

	statusList := room.WorkerStatus()
	expectedList := []Status{expectedStatus1, expectedStatus2}
	require.Equal(t, expectedList, statusList)
}

func TestIsEmpty(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	room := &Room{
		workersMutex: &sync.RWMutex{},
		workers:      []ClientWorker{},
	}

	isEmpty := room.IsEmpty()
	require.True(t, isEmpty)

	mockWorker := NewMockClientWorker(ctrl)
	room.workers = []ClientWorker{mockWorker}
	isEmpty = room.IsEmpty()
	require.False(t, isEmpty)
}

func TestIsPlaylistEmpty(t *testing.T) {
	room := &Room{
		stateMutex: &sync.RWMutex{},
		state: &RoomState{
			playlist: []string{},
		},
	}

	isPlaylistEmpty := room.IsPlaylistEmpty()
	require.True(t, isPlaylistEmpty)

	room.state.playlist = playlist
	isPlaylistEmpty = room.IsPlaylistEmpty()
	require.False(t, isPlaylistEmpty)
}

func TestIsPersistent(t *testing.T) {
	room := &Room{persistent: true}

	isPersistent := room.IsPersistent()
	require.True(t, isPersistent)

	room = &Room{persistent: false}
	isPersistent = room.IsPersistent()
	require.False(t, isPersistent)
}
