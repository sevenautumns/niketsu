package niketsu_server

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
	UnknownType       MessageType = "unknown"
	UnsupportedType   MessageType = "unsupported"
)

type Message interface {
	Type() MessageType
	MarshalMessage() ([]byte, error)
}

func MarshalJSON(m Message) ([]byte, error) {
	marshalledJson, err := json.Marshal(m)
	if err != nil {
		return nil, err
	}

	content := string(marshalledJson)
	content = content[:len(content)-1] + `,"type":"` + string(m.Type()) + `"}`
	return []byte(content), nil
}

type Ping struct {
	Uuid string `json:"uuid"`
}

func (p *Ping) Type() MessageType               { return PingType }
func (p *Ping) MarshalMessage() ([]byte, error) { return MarshalJSON(p) }

type VideoStatus struct {
	Filename *string `json:"filename"`
	Position *uint64 `json:"position"`
	Paused   bool    `json:"paused"`
	Username string  `json:"username"`
}

func (vs *VideoStatus) Type() MessageType               { return VideoStatusType }
func (vs *VideoStatus) MarshalMessage() ([]byte, error) { return MarshalJSON(vs) }

type Pause struct {
	Filename string `json:"filename"`
	Username string `json:"username"`
}

func (p *Pause) Type() MessageType               { return PauseType }
func (p *Pause) MarshalMessage() ([]byte, error) { return MarshalJSON(p) }

type Start struct {
	Filename string `json:"filename"`
	Username string `json:"username"`
}

func (s *Start) Type() MessageType               { return StartType }
func (s *Start) MarshalMessage() ([]byte, error) { return MarshalJSON(s) }

type Seek struct {
	Filename string `json:"filename"`
	Position uint64 `json:"position"`
	Paused   bool   `json:"paused"`
	Username string `json:"username"`
}

func (s *Seek) Type() MessageType               { return SeekType }
func (s *Seek) MarshalMessage() ([]byte, error) { return MarshalJSON(s) }

type Select struct {
	Filename *string `json:"filename"`
	Username string  `json:"username"`
}

func (s *Select) Type() MessageType               { return SelectType }
func (s *Select) MarshalMessage() ([]byte, error) { return MarshalJSON(s) }

type UserMessage struct {
	Message  string `json:"message"`
	Username string `json:"username"`
}

func (m *UserMessage) Type() MessageType               { return UserMessageType }
func (m *UserMessage) MarshalMessage() ([]byte, error) { return MarshalJSON(m) }

type Playlist struct {
	Playlist []string `json:"playlist"`
	Username string   `json:"username"`
}

func (pl *Playlist) Type() MessageType               { return PlaylistType }
func (pl *Playlist) MarshalMessage() ([]byte, error) { return MarshalJSON(pl) }

type Status struct {
	Ready    bool   `json:"ready"`
	Username string `json:"username"`
}

func (s *Status) Type() MessageType               { return StatusType }
func (s *Status) MarshalMessage() ([]byte, error) { return MarshalJSON(s) }

type StatusList struct {
	Rooms    map[string][]Status `json:"rooms"`
	Username string              `json:"username"`
}

func (sl *StatusList) Type() MessageType               { return StatusListType }
func (sl *StatusList) MarshalMessage() ([]byte, error) { return MarshalJSON(sl) }

type Join struct {
	Password string `json:"password"`
	Room     string `json:"room"`
	Username string `json:"username"`
}

func (j *Join) Type() MessageType               { return JoinType }
func (j *Join) MarshalMessage() ([]byte, error) { return MarshalJSON(j) }

type ServerMessage struct {
	Message string `json:"message"`
	IsError bool   `json:"error"`
}

func (sm *ServerMessage) Type() MessageType               { return ServerMessageType }
func (sm *ServerMessage) MarshalMessage() ([]byte, error) { return MarshalJSON(sm) }

type Unknown struct {
	Username string `json:"username"`
	json.RawMessage
}

func (u *Unknown) Type() MessageType               { return UnknownType }
func (u *Unknown) MarshalMessage() ([]byte, error) { return MarshalJSON(u) }

type Unsupported struct {
	Username string `json:"username"`
	json.RawMessage
}

func (u *Unsupported) Type() MessageType               { return UnsupportedType }
func (u *Unsupported) MarshalMessage() ([]byte, error) { return MarshalJSON(u) }

func UnmarshalMessage(data []byte) (Message, error) {
	var t struct {
		Type MessageType `json:"type"`
	}

	if err := json.Unmarshal(data, &t); err != nil {
		return nil, err
	}

	var (
		m   Message
		err error
	)

	switch t.Type {
	case PingType:
		m = &Ping{}
	case VideoStatusType:
		m = &VideoStatus{}
	case StartType:
		m = &Start{}
	case SeekType:
		m = &Seek{}
	case SelectType:
		m = &Select{}
	case UserMessageType:
		m = &UserMessage{}
	case PlaylistType:
		m = &Playlist{}
	case StatusType:
		m = &Status{}
	case StatusListType:
		m = &StatusList{}
	case PauseType:
		m = &Pause{}
	case JoinType:
		m = &Join{}
	case UnsupportedType:
		m = &Unsupported{}
	default:
		m = &Unknown{}
	}

	err = json.Unmarshal(data, &m)

	if err != nil {
		return nil, err
	}

	return m, nil
}
