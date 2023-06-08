package communication

import (
	"testing"

	"github.com/stretchr/testify/require"
)

const joinMessage string = `{"password": "1234", "room": "test", "username": "testUser", "type": "join"}`

func TestUnmarshal(t *testing.T) {
	join, err := UnmarshalMessage([]byte(joinMessage))
	require.NoError(t, err)
	require.Equal(t, JoinType, join.Type())
	require.IsType(t, &Join{}, join)
}
