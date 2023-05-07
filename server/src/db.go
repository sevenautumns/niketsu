package niketsu_server

import (
	"encoding/binary"
	"encoding/json"
	"time"

	bolt "go.etcd.io/bbolt"
)

type Karen struct {
	path     string
	timeout  time.Duration
	statFreq time.Duration
	db       bolt.DB
}

// Creates a new Database connection with some additional options. Make sure to call defer karen.Close() after successfully initializing the database. timeout and statFreq are given in seconds.
func NewKaren(path string, timeout uint64, statFreq uint64) (*Karen, error) {
	var karen Karen
	karen.path = path
	karen.timeout = time.Duration(timeout * uint64(time.Second))
	karen.statFreq = time.Duration(statFreq * uint64(time.Second))

	db, err := bolt.Open(path, 0600, nil)
	if err != nil {
		return nil, err
	}
	karen.db = *db

	return &karen, nil
}

func (karen *Karen) Close() {
	karen.db.Close()
}

func (karen *Karen) Monitor() {
	prev := karen.db.Stats()

	for {
		time.Sleep(karen.statFreq)

		stats := karen.db.Stats()
		diff := stats.Sub(&prev)
		encoded, err := json.Marshal(diff)
		if err != nil {
			logger.Warnw("Error occured creating stats diff", "diff", encoded)
		} else {
			logger.Infow("Current stats", "stats", encoded)
		}

		prev = stats
	}
}

func (karen *Karen) Update(bucket string, playlist []byte, video string, position uint64) {
	err := karen.db.Update(func(tx *bolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		err = b.Put([]byte("video"), []byte(video))
		if err != nil {
			return err
		}

		pos := make([]byte, 8)
		binary.LittleEndian.PutUint64(pos, position)
		err = b.Put([]byte("position"), pos)
		if err != nil {
			return err
		}

		err = b.Put([]byte("playlist"), playlist)
		if err != nil {
			return err
		}

		return nil
	})

	if err != nil {
		logger.Warnw("Update transaction failed", "error", err)
	}
}

func (karen *Karen) Get(bucket string, key string) ([]byte, error) {
	var val []byte

	err := karen.db.View(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucket))

		if b == nil {
			return bolt.ErrBucketNotFound
		}
		val = b.Get([]byte(key))

		return nil
	})

	if err != nil {
		return nil, err
	}

	return val, nil
}

func (karen *Karen) DeleteBucket(bucket string) error {
	err := karen.db.Update(func(tx *bolt.Tx) error {
		err := tx.DeleteBucket([]byte(bucket))

		return err
	})

	return err
}

func (karen *Karen) DeleteKey(bucket string, key string) error {
	err := karen.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucket))
		err := b.Delete([]byte(key))

		return err
	})

	return err
}
