package communication

import (
	"fmt"
	"log"
	"testing"

	"github.com/stretchr/testify/require"
)

var (
	testUuid              string   = "550e8400-e29b-41d4-a716-446655440000"
	testFilename          string   = "testFile"
	testPosition          uint64   = 0
	testPaused            bool     = false
	testSpeed             float64  = 1.0
	testReady             bool     = true
	testNotReady          bool     = false
	testUsername          string   = "testUser"
	testUsername2         string   = "testUser2"
	testDesync            bool     = false
	testMessage           string   = "testMessage"
	testError             bool     = false
	testPlaylist          []string = []string{"first", "second"}
	testPassword          string   = "1234"
	testRoom              string   = "testRoom"
	testCommand           string   = "testCommand1"
	testCommandValue      string   = "testValue"
	testOtherCommand      string   = "testCommand2"
	testOtherCommandValue bool     = false
	pingMessage           string   = fmt.Sprintf(`{"uuid":"%s","type":"ping"}`, testUuid)
	videoStatusMessage    string   = fmt.Sprintf(`{"filename":"%s","position":%d,"paused":%t,"speed":%g,"username":"%s","type":"videoStatus"}`,
		testFilename, testPosition, testPaused, testSpeed, testUsername)
	statusListMessage string = fmt.Sprintf(`{"rooms":{"room1":[{"ready":%t,"username":"%s"},{"ready":%t,"username":"%s"}],"room2":[]},"type":"statusList"}`,
		testNotReady, testUsername, testReady, testUsername2)
	pauseMessage string = fmt.Sprintf(`{"username":"%s","type":"pause"}`, testUsername)
	startMessage string = fmt.Sprintf(`{"username":"%s","type":"start"}`, testUsername)
	seekMessage  string = fmt.Sprintf(`{"filename":"%s","position":%d,"speed":%g,"paused":%t,"desync":%t,"username":"%s","type":"seek"}`,
		testFilename, testPosition, testSpeed, testPaused, testDesync, testUsername)
	selectMessage        string = fmt.Sprintf(`{"filename":"%s","username":"%s","type":"select"}`, testFilename, testUsername)
	userMessage          string = fmt.Sprintf(`{"message":"%s","username":"%s","type":"userMessage"}`, testMessage, testUsername)
	serverMessage        string = fmt.Sprintf(`{"message":"%s","error":%t,"type":"serverMessage"}`, testMessage, testError)
	playlistMessage      string = fmt.Sprintf(`{"playlist":["%s","%s"],"username":"%s","type":"playlist"}`, testPlaylist[0], testPlaylist[1], testUsername)
	statusMessage        string = fmt.Sprintf(`{"ready":%t,"username":"%s","type":"status"}`, testReady, testUsername)
	joinMessage          string = fmt.Sprintf(`{"password":"%s","room":"%s","username":"%s","type":"join"}`, testPassword, testRoom, testUsername)
	playbackSpeedMessage string = fmt.Sprintf(`{"speed":%g,"username":"%s","type":"playbackSpeed"}`, testSpeed, testUsername)
	unknownMessage       string = fmt.Sprintf(`{"username":"%s","%s":"%s"}`, testUsername, testCommand, testCommandValue)
	unsupportedMessage   string = fmt.Sprintf(`{"username":"%s","%s":%t,"type":"unsupported"}`, testUsername, testOtherCommand, testOtherCommandValue)
	failedFormatMessage  string = `this may not be: a json {}`
	failedFormatMessage2 string = `{"type":"wrong","failed":true}`
)

func TestUnmarshal(t *testing.T) {
	join, err := UnmarshalMessage([]byte(joinMessage))
	testType(t, JoinType, &Join{}, join, err)

	ping, err := UnmarshalMessage([]byte(pingMessage))
	testType(t, PingType, &Ping{}, ping, err)

	videoStatus, err := UnmarshalMessage([]byte(videoStatusMessage))
	testType(t, VideoStatusType, &VideoStatus{}, videoStatus, err)

	statusList, err := UnmarshalMessage([]byte(statusListMessage))
	testType(t, StatusListType, &StatusList{}, statusList, err)

	pause, err := UnmarshalMessage([]byte(pauseMessage))
	testType(t, PauseType, &Pause{}, pause, err)

	start, err := UnmarshalMessage([]byte(startMessage))
	testType(t, StartType, &Start{}, start, err)

	seek, err := UnmarshalMessage([]byte(seekMessage))
	testType(t, SeekType, &Seek{}, seek, err)

	sel, err := UnmarshalMessage([]byte(selectMessage))
	testType(t, SelectType, &Select{}, sel, err)

	user, err := UnmarshalMessage([]byte(userMessage))
	testType(t, UserMessageType, &UserMessage{}, user, err)

	server, err := UnmarshalMessage([]byte(serverMessage))
	testType(t, ServerMessageType, &ServerMessage{}, server, err)

	playlist, err := UnmarshalMessage([]byte(playlistMessage))
	testType(t, PlaylistType, &Playlist{}, playlist, err)

	status, err := UnmarshalMessage([]byte(statusMessage))
	testType(t, StatusType, &Status{}, status, err)

	playbackSpeed, err := UnmarshalMessage([]byte(playbackSpeedMessage))
	testType(t, PlaybackSpeedType, &PlaybackSpeed{}, playbackSpeed, err)

	unknown, err := UnmarshalMessage([]byte(unknownMessage))
	testType(t, UnknownType, &Unknown{}, unknown, err)

	unsupported, err := UnmarshalMessage([]byte(unsupportedMessage))
	testType(t, UnsupportedType, &Unsupported{}, unsupported, err)

}

func testType(t *testing.T, messageType MessageType, expectedMessage Message, actualMessage Message, err error) {
	require.NoError(t, err)
	require.Equal(t, messageType, actualMessage.Type())
	require.IsType(t, expectedMessage, actualMessage)
}

func TestFailedUnmarshal(t *testing.T) {
	msg, err := UnmarshalMessage([]byte(failedFormatMessage))
	require.Nil(t, msg)
	require.Error(t, err)

	msg2, err := UnmarshalMessage([]byte(failedFormatMessage2))
	require.IsType(t, &Unknown{}, msg2)
	require.NoError(t, err)
}

func TestMarshal(t *testing.T) {
	join, err := MarshalMessage(Join{
		Password: testPassword,
		Room:     testRoom,
		Username: testUsername,
	})
	testMessageContent(t, []byte(joinMessage), join, err)

	ping, err := MarshalMessage(Ping{
		Uuid: testUuid,
	})
	testMessageContent(t, []byte(pingMessage), ping, err)

	videoStatus, err := MarshalMessage(VideoStatus{
		Filename: &testFilename,
		Position: &testPosition,
		Paused:   testPaused,
		Speed:    testSpeed,
		Username: testUsername,
	})
	testMessageContent(t, []byte(videoStatusMessage), videoStatus, err)

	statusList, err := MarshalMessage(StatusList{
		Rooms: map[string][]Status{
			"room1": {{Username: testUsername, Ready: testNotReady},
				{Username: testUsername2, Ready: testReady}},
			"room2": {}},
	})
	log.Print(statusListMessage)
	log.Print(string(statusList))
	testMessageContent(t, []byte(statusListMessage), statusList, err)

	pause, err := MarshalMessage(Pause{
		Username: testUsername,
	})
	testMessageContent(t, []byte(pauseMessage), pause, err)

	start, err := MarshalMessage(Start{
		Username: testUsername,
	})
	testMessageContent(t, []byte(startMessage), start, err)

	seek, err := MarshalMessage(Seek{
		Filename: testFilename,
		Position: testPosition,
		Speed:    testSpeed,
		Paused:   testPaused,
		Desync:   testDesync,
		Username: testUsername,
	})
	testMessageContent(t, []byte(seekMessage), seek, err)

	sel, err := MarshalMessage(Select{
		Filename: &testFilename,
		Username: testUsername,
	})
	testMessageContent(t, []byte(selectMessage), sel, err)

	user, err := MarshalMessage(UserMessage{
		Message:  testMessage,
		Username: testUsername,
	})
	testMessageContent(t, []byte(userMessage), user, err)

	server, err := MarshalMessage(ServerMessage{
		Message: testMessage,
		IsError: testError,
	})
	testMessageContent(t, []byte(serverMessage), server, err)

	playlist, err := MarshalMessage(Playlist{
		Playlist: testPlaylist,
		Username: testUsername,
	})
	testMessageContent(t, []byte(playlistMessage), playlist, err)

	status, err := MarshalMessage(Status{
		Ready:    testReady,
		Username: testUsername,
	})
	testMessageContent(t, []byte(statusMessage), status, err)

	playbackSpeed, err := MarshalMessage(PlaybackSpeed{
		Speed:    testSpeed,
		Username: testUsername,
	})
	testMessageContent(t, []byte(playbackSpeedMessage), playbackSpeed, err)
}

func testMessageContent(t *testing.T, expectedMessage []byte, actualMessage []byte, err error) {
	require.NoError(t, err)
	require.Equal(t, expectedMessage, actualMessage)
}

func TestFailedMarshal(t *testing.T) {
	unknownMessage := Unknown{testUsername, []byte(failedFormatMessage)}
	payload, err := MarshalMessage(unknownMessage)
	require.Nil(t, payload)
	require.Error(t, err)
}
