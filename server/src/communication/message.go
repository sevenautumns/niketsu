package communication

import (
	"encoding/json"
)

type MessageType string

const (
	PingType          MessageType = "ping"
	VideoStatusType   MessageType = "videoStatus"
	StatusListType    MessageType = "statusList"
	PauseType         MessageType = "pause"
	StartType         MessageType = "start"
	SeekType          MessageType = "seek"
	SelectType        MessageType = "select"
	UserMessageType   MessageType = "userMessage"
	ServerMessageType MessageType = "serverMessage"
	PlaylistType      MessageType = "playlist"
	StatusType        MessageType = "status"
	JoinType          MessageType = "join"
	PlaybackSpeedType MessageType = "playbackSpeed"
	UnknownType       MessageType = "unknown"
	UnsupportedType   MessageType = "unsupported"
)

type Message interface {
	Type() MessageType
}

type Ping struct {
	Uuid string `json:"uuid"`
}

func (p Ping) Type() MessageType { return PingType }

type VideoStatus struct {
	Filename *string `json:"filename"`
	Position *uint64 `json:"position"`
	Paused   bool    `json:"paused"`
	Speed    float64 `json:"speed"`
	Username string  `json:"username"`
}

func (vs VideoStatus) Type() MessageType { return VideoStatusType }

type Pause struct {
	Username string `json:"username"`
}

func (p Pause) Type() MessageType { return PauseType }

type Start struct {
	Username string `json:"username"`
}

func (s Start) Type() MessageType { return StartType }

type Seek struct {
	Filename string  `json:"filename"`
	Position uint64  `json:"position"`
	Speed    float64 `json:"speed"`
	Paused   bool    `json:"paused"`
	Desync   bool    `json:"desync"`
	Username string  `json:"username"`
}

func (s Seek) Type() MessageType { return SeekType }

type Select struct {
	Filename *string `json:"filename"`
	Username string  `json:"username"`
}

func (s Select) Type() MessageType { return SelectType }

type UserMessage struct {
	Message  string `json:"message"`
	Username string `json:"username"`
}

func (m UserMessage) Type() MessageType { return UserMessageType }

type Playlist struct {
	Playlist []string `json:"playlist"`
	Username string   `json:"username"`
}

func (pl Playlist) Type() MessageType { return PlaylistType }

type Status struct {
	Ready    bool   `json:"ready"`
	Username string `json:"username"`
}

func (s Status) Type() MessageType { return StatusType }

type StatusList struct {
	Rooms map[string][]Status `json:"rooms"`
}

func (sl StatusList) Type() MessageType { return StatusListType }

type Join struct {
	Password string `json:"password"`
	Room     string `json:"room"`
	Username string `json:"username"`
}

func (j Join) Type() MessageType { return JoinType }

type ServerMessage struct {
	Message string `json:"message"`
	IsError bool   `json:"error"`
}

func (sm ServerMessage) Type() MessageType { return ServerMessageType }

type PlaybackSpeed struct {
	Speed    float64 `json:"speed"`
	Username string  `json:"username"`
}

func (pl PlaybackSpeed) Type() MessageType { return PlaybackSpeedType }

type Unknown struct {
	Username string `json:"username"`
	json.RawMessage
}

func (u Unknown) Type() MessageType { return UnknownType }

type Unsupported struct {
	Username string `json:"username"`
	json.RawMessage
}

func (u Unsupported) Type() MessageType { return UnsupportedType }

func UnmarshalMessage(data []byte) (Message, error) {
	message, err := getMessage(data)
	if err != nil {
		return nil, err
	}

	// Due to the Unknown message, we can deliberately parse all jsons.
	// Hence, this will not fail
	json.Unmarshal(data, &message)

	return message, nil
}

func getMessage(data []byte) (Message, error) {
	var messageHead struct {
		Type MessageType `json:"type"`
	}
	if err := json.Unmarshal(data, &messageHead); err != nil {
		return nil, err
	}

	var message Message
	switch messageHead.Type {
	case PingType:
		message = &Ping{}
	case VideoStatusType:
		message = &VideoStatus{}
	case StartType:
		message = &Start{}
	case SeekType:
		message = &Seek{}
	case SelectType:
		message = &Select{}
	case UserMessageType:
		message = &UserMessage{}
	case PlaylistType:
		message = &Playlist{}
	case StatusType:
		message = &Status{}
	case StatusListType:
		message = &StatusList{}
	case PauseType:
		message = &Pause{}
	case JoinType:
		message = &Join{}
	case PlaybackSpeedType:
		message = &PlaybackSpeed{}
	case UnsupportedType:
		message = &Unsupported{}
	case ServerMessageType:
		message = &ServerMessage{}
	default:
		message = &Unknown{}
	}

	return message, nil
}

func MarshalMessage(message Message) ([]byte, error) {
	encodedMessage, err := json.Marshal(message)
	if err != nil {
		return nil, err
	}

	appendedMessage := appendType(encodedMessage, message.Type())
	return appendedMessage, nil
}

func appendType(encodedMessage []byte, messageType MessageType) []byte {
	appendedMessage := string(encodedMessage)
	appendedMessage = appendedMessage[:len(appendedMessage)-1] + `,"type":"` + string(messageType) + `"}`
	return []byte(appendedMessage)
}
