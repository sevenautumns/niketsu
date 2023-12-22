package communication

import (
	"testing"

	uuid "github.com/google/uuid"
	"github.com/stretchr/testify/require"
)

func TestSetCache(t *testing.T) {
	tracker := NewCacheTracker()
	uuid1 := uuid.New()
	tracker.SetCache(uuid1, true)
	require.Equal(t, uint64(1), tracker.length)
	require.Equal(t, uint64(1), tracker.trueCount)
	require.Len(t, tracker.cacheStatus, 1)
	require.Contains(t, tracker.cacheStatus, uuid1)

	uuid2 := uuid.New()
	tracker.SetCache(uuid2, false)
	require.Equal(t, uint64(2), tracker.length)
	require.Equal(t, uint64(1), tracker.trueCount)
	require.Len(t, tracker.cacheStatus, 2)
	require.Contains(t, tracker.cacheStatus, uuid2)
}

func TestDeleteCache(t *testing.T) {
	tracker := CacheTracker{
		cacheStatus: make(map[uuid.UUID]bool),
		trueCount:   0,
		length:      0,
	}

	uuid1 := uuid.New()
	tracker.DeleteCache(uuid1)

	require.Equal(t, uint64(0), tracker.length)
	require.Equal(t, uint64(0), tracker.trueCount)
	require.Len(t, tracker.cacheStatus, 0)

	uuid2 := uuid.New()
	tracker = CacheTracker{
		cacheStatus: map[uuid.UUID]bool{
			uuid1: true,
			uuid2: false,
		},
		length:    2,
		trueCount: 1,
	}

	tracker.DeleteCache(uuid1)
	require.Equal(t, uint64(1), tracker.length)
	require.Equal(t, uint64(0), tracker.trueCount)
	require.Len(t, tracker.cacheStatus, 1)
}

func TestReset(t *testing.T) {
	uuid1 := uuid.New()
	uuid2 := uuid.New()
	tracker := CacheTracker{
		cacheStatus: map[uuid.UUID]bool{
			uuid1: true,
			uuid2: true,
		},
		trueCount: 2,
		length:    2,
	}

	tracker.Reset()
	require.Equal(t, uint64(2), tracker.length)
	require.Equal(t, uint64(0), tracker.trueCount)
	require.Len(t, tracker.cacheStatus, 2)
}

func TestCacheFull(t *testing.T) {
	tracker := NewCacheTracker()
	require.False(t, tracker.CacheFull())

	uuid1 := uuid.New()
	uuid2 := uuid.New()
	tracker = &CacheTracker{
		cacheStatus: map[uuid.UUID]bool{
			uuid1: true,
			uuid2: true,
		},
		trueCount: 2,
		length:    2,
	}

	require.True(t, tracker.CacheFull())

}
