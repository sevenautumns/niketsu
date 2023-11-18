package communication

import (
	"testing"
	"time"

	uuid "github.com/google/uuid"
	"github.com/stretchr/testify/require"
)

func TestCacheHeapMapNew(t *testing.T) {
	chm := NewCacheHeapMap()
	require.Empty(t, chm.cacheHeap)
	require.Empty(t, chm.cacheMap)
	require.NotNil(t, chm.cacheHeap)
	require.NotNil(t, chm.cacheMap)
}

func TestCacheHeapMapUpdate(t *testing.T) {
	chm := NewCacheHeapMap()
	chm.Update(uuid.New(), Duration{time.Second})
	require.Len(t, chm.cacheMap, 1)
	require.Len(t, chm.cacheHeap, 1)

	chm.Update(uuid.New(), Duration{time.Second})
	require.Len(t, chm.cacheMap, 2)
	require.Len(t, chm.cacheHeap, 2)
}

func TestCacheHeapMapPeekMin(t *testing.T) {
	values := []int{1, 5, 0, 10, 20, 7}
	chm := NewCacheHeapMap()
	require.Nil(t, chm.PeekMin())

	for _, v := range values {
		new_uuid := uuid.New()
		chm.Update(new_uuid, DurationFromUint64(uint64(v)))
	}

	require.Equal(t, Duration{0}, *chm.PeekMin())
}

func TestCacheHeapMapRemove(t *testing.T) {
	values := []uint64{1, 5, 0}
	uuids := []uuid.UUID{uuid.New(), uuid.New(), uuid.New()}
	chm := NewCacheHeapMap()

	for i, v := range values {
		chm.Update(uuids[i], DurationFromUint64(v))
	}

	chm.Remove(uuids[2])
	require.Len(t, chm.cacheHeap, 2)
	require.Len(t, chm.cacheMap, 2)
	require.Equal(t, Duration{1}, *chm.PeekMin())

	chm.Remove(uuids[0])
	require.Len(t, chm.cacheHeap, 1)
	require.Len(t, chm.cacheMap, 1)
	require.Equal(t, Duration{5}, *chm.PeekMin())
}
