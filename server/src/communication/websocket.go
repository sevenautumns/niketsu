package communication

import (
	"net"

	"github.com/gobwas/ws"
	"github.com/gobwas/ws/wsutil"
)

type OpCode interface {
	IsContinuation() bool
	IsText() bool
	IsBinary() bool
	IsClose() bool
	IsPing() bool
	IsPong() bool
	IsControl() bool
	IsData() bool
	IsReserved() bool
}

type WsOpCode struct {
	code ws.OpCode
}

func (opCode WsOpCode) IsContinuation() bool {
	return opCode.code == ws.OpContinuation
}

func (opCode WsOpCode) IsText() bool {
	return opCode.code == ws.OpText
}

func (opCode WsOpCode) IsBinary() bool {
	return opCode.code == ws.OpBinary
}

func (opCode WsOpCode) IsClose() bool {
	return opCode.code == ws.OpClose
}

func (opCode WsOpCode) IsPing() bool {
	return opCode.code == ws.OpPing
}

func (opCode WsOpCode) IsPong() bool {
	return opCode.code == ws.OpPong
}

func (opCode WsOpCode) IsControl() bool {
	return opCode.code.IsControl()
}

func (opCode WsOpCode) IsData() bool {
	return opCode.code.IsData()
}

func (opCode WsOpCode) IsReserved() bool {
	return opCode.code.IsReserved()
}

type WebSocket interface {
	WriteMessage(message []byte) error
	ReadMessage() ([]byte, OpCode, error)
	Close() error
}

type WsWebSocket struct {
	conn net.Conn
}

// TODO low level api implementation
// Currently, only writing text is supported.
func (webSocket WsWebSocket) WriteMessage(message []byte) error {
	err := wsutil.WriteServerMessage(webSocket.conn, ws.OpText, message)
	return err
}
func (webSocket WsWebSocket) ReadMessage() ([]byte, OpCode, error) {
	conn, op, err := wsutil.ReadClientData(webSocket.conn)
	opCode := WsOpCode{code: op}
	return conn, opCode, err
}

func (webSocket WsWebSocket) Close() error {
	return webSocket.conn.Close()
}
