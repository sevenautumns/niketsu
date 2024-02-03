package communication

import uuid "github.com/google/uuid"

type CacheTracker struct {
	cacheStatus map[uuid.UUID]bool
	trueCount   uint64
	length      uint64
}

func NewCacheTracker() *CacheTracker {
	tracker := CacheTracker{
		cacheStatus: make(map[uuid.UUID]bool),
		trueCount:   0,
		length:      0,
	}
	return &tracker
}

func (ct *CacheTracker) SetCache(id uuid.UUID, cache bool) {
	currentState, exists := ct.cacheStatus[id]
	if !exists {
		ct.cacheStatus[id] = cache
		if cache {
			ct.trueCount++
		}
		ct.length++
		return
	}

	if cache && !currentState {
		ct.trueCount++
	} else if !cache && currentState {
		ct.trueCount--
	}

	ct.cacheStatus[id] = cache
}

func (ct *CacheTracker) DeleteCache(id uuid.UUID) {
	currentState, exists := ct.cacheStatus[id]
	if !exists {
		return
	}

	if currentState {
		ct.trueCount--
	}

	delete(ct.cacheStatus, id)
	ct.length--
}

func (ct *CacheTracker) Reset() {
	cs := ct.cacheStatus
	for key := range cs {
		cs[key] = false
	}
	ct.trueCount = 0
}

func (ct *CacheTracker) CacheFull() bool {
	return ct.length != 0 && ct.length == ct.trueCount
}
