package communication

import (
	"encoding/json"
	"errors"
	"time"
)

type Duration struct {
	time.Duration
}

func (d Duration) MarshalJSON() ([]byte, error) {
	return json.Marshal(d.uint64())
}

func (d *Duration) UnmarshalJSON(b []byte) error {
	var v interface{}
	if err := json.Unmarshal(b, &v); err != nil {
		return err
	}
	switch value := v.(type) {
	case float64:
		d.Duration = time.Duration(value)
		return nil
	case int:
		d.Duration = time.Duration(value)
		return nil
	case string:
		var err error
		d.Duration, err = time.ParseDuration(value)
		if err != nil {
			return err
		}
		return nil
	default:
		return errors.New("invalid duration")
	}
}

func (d Duration) mult(factor float64) Duration {
	return Duration{time.Duration(float64(d.Duration) * factor)}
}

func (d Duration) add(duration Duration) Duration {
	return Duration{d.Duration + duration.Duration}
}

func (d Duration) sub(duration Duration) Duration {
	return Duration{d.Duration - duration.Duration}
}

func (d Duration) div(value uint64) Duration {
	return Duration{d.Duration / time.Duration(value)}
}

func (d Duration) negate() Duration {
	return Duration{-d.Duration}
}

func (d Duration) greater(duration Duration) bool {
	return d.Duration > duration.Duration
}

func (d Duration) smaller(duration Duration) bool {
	return d.Duration < duration.Duration
}

func (d Duration) equal(duration Duration) bool {
	return d.Duration == duration.Duration
}

func (d Duration) uint64() uint64 {
	return uint64(d.Duration)
}

func durationFromUint64(i uint64) Duration {
	return Duration{time.Duration(i)}
}

func timeSince(t time.Time) Duration {
	return Duration{time.Since(t)}
}

func timeSub(t time.Time, otherT time.Time) Duration {
	return Duration{t.Sub(otherT)}
}

func timeAdd(t time.Time, duration Duration) time.Time {
	return t.Add(duration.Duration)
}
