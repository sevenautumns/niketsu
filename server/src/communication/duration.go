package communication

import (
	"encoding/json"
	"errors"
	"time"
)

// Expect time in milliseconds
type Duration struct {
	time.Duration
}

func (d Duration) MarshalJSON() ([]byte, error) {
	durMillis := d.Div(uint64(time.Millisecond)).Uint64()
	durMillis = max(durMillis, 0)
	return json.Marshal(durMillis)
}

func (d *Duration) UnmarshalJSON(b []byte) error {
	var v interface{}
	if err := json.Unmarshal(b, &v); err != nil {
		return err
	}
	switch value := v.(type) {
	case float64:
		d.Duration = time.Duration(float64(time.Millisecond) * value)
		return nil
	case int:
		d.Duration = time.Duration(int(time.Millisecond) * value)
		return nil
	case string:
		var err error
		d.Duration, err = time.ParseDuration(value)
		if err != nil {
			return err
		}
		d.MultInt(int(time.Millisecond))
		return nil
	default:
		return errors.New("invalid duration")
	}
}

func (d Duration) MultFloat64(factor float64) Duration {
	return Duration{time.Duration(float64(d.Duration) * factor)}
}

func (d Duration) MultInt(factor int) Duration {
	return Duration{time.Duration(int(d.Duration) * factor)}
}

func (d Duration) Add(duration Duration) Duration {
	return Duration{d.Duration + duration.Duration}
}

func (d Duration) Sub(duration Duration) Duration {
	return Duration{d.Duration - duration.Duration}
}

func (d Duration) Div(value uint64) Duration {
	return Duration{d.Duration / time.Duration(value)}
}

func (d Duration) Negate() Duration {
	return Duration{-d.Duration}
}

func (d Duration) Greater(duration Duration) bool {
	return d.Duration > duration.Duration
}

func (d Duration) Smaller(duration Duration) bool {
	return d.Duration < duration.Duration
}

func (d Duration) Equal(duration Duration) bool {
	return d.Duration == duration.Duration
}

func (d Duration) Uint64() uint64 {
	return uint64(d.Duration)
}

func DurationFromUint64(i uint64) Duration {
	return Duration{time.Duration(i)}
}

func TimeSince(t time.Time) Duration {
	return Duration{time.Since(t)}
}

func TimeSub(t time.Time, otherT time.Time) Duration {
	return Duration{t.Sub(otherT)}
}

func TimeAdd(t time.Time, duration Duration) time.Time {
	return t.Add(duration.Duration)
}

func MinDuration(x, y Duration) Duration {
	return Duration{min(x.Duration, y.Duration)}
}
