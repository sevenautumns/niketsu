package communication

import (
	"container/heap"

	uuid "github.com/google/uuid"
)

type Cache struct {
	ID    uuid.UUID
	Value Duration
	Index int
}

type CacheHeap []*Cache

type CacheHeapMap struct {
	cacheHeap CacheHeap
	cacheMap  map[uuid.UUID]*Cache
}

func (c CacheHeap) Len() int           { return len(c) }
func (c CacheHeap) Less(i, j int) bool { return c[i].Value.Smaller(c[j].Value) }
func (c CacheHeap) Swap(i, j int) {
	c[i], c[j] = c[j], c[i]
	c[i].Index = i
	c[j].Index = j
}

func (c *CacheHeap) Push(x interface{}) {
	n := len(*c)
	item := x.(*Cache)
	item.Index = n
	*c = append(*c, item)
}

func (c *CacheHeap) Pop() interface{} {
	old := *c
	n := len(old)
	item := old[n-1]
	item.Index = -1
	*c = old[0 : n-1]
	return item
}

func NewCacheHeapMap() CacheHeapMap {
	cacheMap := make(map[uuid.UUID]*Cache, 0)
	cacheHeap := make(CacheHeap, 0)
	heap.Init(&cacheHeap)
	return CacheHeapMap{cacheHeap, cacheMap}
}

func (c *CacheHeapMap) Update(ID uuid.UUID, value Duration) {
	if cache, exists := c.cacheMap[ID]; !exists {
		cache := &Cache{
			ID:    ID,
			Value: value,
		}
		heap.Push(&c.cacheHeap, cache)
		c.cacheMap[ID] = cache
	} else {
		cache.Value = value
		heap.Fix(&c.cacheHeap, cache.Index)
	}
}

func (c CacheHeapMap) PeekMin() *Duration {
	if c.cacheHeap.Len() == 0 {
		return nil
	}

	return &c.cacheHeap[0].Value
}

func (c *CacheHeapMap) Remove(ID uuid.UUID) {
	if cache, exists := c.cacheMap[ID]; exists {
		heap.Remove(&c.cacheHeap, cache.Index)
		delete(c.cacheMap, ID)
	}
}
