package communication

import (
	"testing"
	"time"

	"github.com/stretchr/testify/require"
)

func TestDurationMarshalJSON(t *testing.T) {
	dur := Duration{0}
	marshalled, err := dur.MarshalJSON()
	require.NoError(t, err)
	require.Equal(t, []byte("0"), marshalled)

	dur = Duration{time.Duration(10000 * time.Millisecond)}
	marshalled, err = dur.MarshalJSON()
	require.NoError(t, err)
	require.Equal(t, []byte("10000"), marshalled)
}

func TestDurationUnmarshalJSON(t *testing.T) {
	var dur Duration
	err := dur.UnmarshalJSON([]byte("0"))
	require.NoError(t, err)
	require.Equal(t, time.Nanosecond*0, dur.Duration)

	err = dur.UnmarshalJSON([]byte("10000"))
	require.NoError(t, err)
	require.Equal(t, time.Millisecond*10000, dur.Duration)
}

func TestDurationMultFloat64(t *testing.T) {
	dur1 := Duration{time.Millisecond * 10}
	factor := float64(5)
	mult := dur1.MultFloat64(factor)
	require.Equal(t, time.Millisecond*50, mult.Duration)

	factor = float64(0)
	mult = dur1.MultFloat64(factor)
	require.Equal(t, time.Millisecond*0, mult.Duration)
}

func TestDurationMultInt(t *testing.T) {
	dur1 := Duration{time.Millisecond * 10}
	factor := 5
	mult := dur1.MultInt(factor)
	require.Equal(t, time.Millisecond*50, mult.Duration)

	factor = 0
	mult = dur1.MultInt(factor)
	require.Equal(t, time.Millisecond*0, mult.Duration)
}

func TestDurationAdd(t *testing.T) {
	dur1 := Duration{time.Millisecond * 10}
	dur2 := Duration{time.Millisecond * 5}
	add := dur1.Add(dur2)
	require.Equal(t, time.Millisecond*15, add.Duration)

	dur1 = Duration{time.Millisecond * 10}
	dur2 = Duration{time.Millisecond * 0}
	add = dur1.Add(dur2)
	require.Equal(t, time.Millisecond*10, add.Duration)
}

func TestDurationSub(t *testing.T) {
	dur1 := Duration{time.Millisecond * 10}
	dur2 := Duration{time.Millisecond * 5}
	sub := dur1.Sub(dur2)
	require.Equal(t, time.Millisecond*5, sub.Duration)

	dur1 = Duration{time.Millisecond * 10}
	dur2 = Duration{time.Millisecond * 0}
	sub = dur1.Add(dur2)
	require.Equal(t, time.Millisecond*10, sub.Duration)
}

func TestDurationDiv(t *testing.T) {
	dur1 := Duration{time.Millisecond * 10}
	denom := uint64(5)
	div := dur1.Div(denom)
	require.Equal(t, time.Millisecond*2, div.Duration)

	dur1 = Duration{time.Millisecond * 10}
	denom = uint64(10)
	div = dur1.Div(denom)
	require.Equal(t, time.Millisecond, div.Duration)
}

func TestDurationNegate(t *testing.T) {
	dur := Duration{time.Millisecond}
	neg := dur.Negate()
	require.Equal(t, -time.Millisecond, neg.Duration)
}

func TestDurationGreater(t *testing.T) {
	dur1 := Duration{time.Millisecond}
	dur2 := Duration{time.Millisecond * 2}
	smaller := dur1.Greater(dur2)
	require.False(t, smaller)

	greater := dur2.Greater(dur1)
	require.True(t, greater)
}

func TestDurationSmaller(t *testing.T) {
	dur1 := Duration{time.Millisecond * 10}
	dur2 := Duration{time.Millisecond}
	greater := dur1.Smaller(dur2)
	require.False(t, greater)

	smaller := dur2.Smaller(dur1)
	require.True(t, smaller)
}

func TestDurationEqual(t *testing.T) {
	dur1 := Duration{time.Millisecond * 10}
	dur2 := Duration{time.Millisecond}
	unequal := dur1.Equal(dur2)
	require.False(t, unequal)

	unequal = dur2.Equal(dur1)
	require.False(t, unequal)

	equal := dur1.Equal(dur1)
	require.True(t, equal)

	dur1 = Duration{time.Millisecond}
	dur2 = Duration{time.Millisecond}
	equal = dur1.Equal(dur2)
	require.True(t, equal)
}

func TestDurationUint64(t *testing.T) {
	dur := Duration{time.Millisecond}
	d := dur.Uint64()
	require.Equal(t, uint64(1e6), d)
}

func TestDurationTimeSince(t *testing.T) {
	t1 := time.Now().Add(-time.Minute)
	dur := TimeSince(t1)
	require.True(t, dur.Duration > 0)
	require.True(t, dur.Duration < time.Minute+10*time.Second && dur.Duration >= time.Minute)
}

func TestDurationTimeSub(t *testing.T) {
	t1 := time.Now()
	t2 := t1.Add(-time.Hour)
	dur := TimeSub(t1, t2)
	require.Equal(t, time.Hour, dur.Duration)
}

func TestDurationTimeAdd(t *testing.T) {
	t1 := time.Now()
	dur := Duration{time.Hour}
	result := TimeAdd(t1, dur)
	require.Equal(t, t1.Add(time.Hour).Unix(), result.Unix())
}

func TestMinDuration(t *testing.T) {
	smallDuration := Duration{42 * time.Second}
	largerDuration := Duration{96 * time.Minute}
	maxDuration := MinDuration(smallDuration, largerDuration)
	require.Equal(t, Duration{42 * time.Second}, maxDuration)
}
