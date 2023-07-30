package db

import (
	"encoding/binary"
	"errors"
	"os"
	"time"

	"go.etcd.io/bbolt"
)

const (
	PlaylistKey = "playlist"
	VideoKey    = "video"
	PositionKey = "position"
)

// TODO updateplaylist -> update abitrary map elements
// Functionality for updating the states. Buckets and keys are only created
// based on the Update() function.
type DBManager interface {
	Open() error
	Close() error
	Delete() error
	Update(bucket string, key string, value []byte) error
	GetValue(bucket string, key string) ([]byte, error)
	DeleteKey(bucket string, key string) error
	DeleteBucket(bucket string) error
	UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) error
	GetAll(bucket string) (map[string][]byte, error)
}

type BoltKeyValueStore struct {
	db      *bbolt.DB
	path    string
	timeout time.Duration
}

func NewDBManager(path string, timeout uint64) (DBManager, error) {
	if path == "" || timeout == 0 {
		return nil, errors.New("invalid parameters. path should not be empty and timeout should be non-zero.")
	}

	return &BoltKeyValueStore{path: path, timeout: time.Duration(timeout * uint64(time.Second))}, nil
}

func (keyValueStore *BoltKeyValueStore) Open() error {
	conn, err := bbolt.Open(keyValueStore.path, 0600, &bbolt.Options{Timeout: keyValueStore.timeout})
	if err != nil {
		return err
	}
	keyValueStore.db = conn

	return nil
}

func (keyValueStore *BoltKeyValueStore) Close() error {
	if keyValueStore.db == nil {
		return errors.New("Database not initialized. Can not call Close()")
	}

	return keyValueStore.db.Close()
}

func (keyValueStore *BoltKeyValueStore) Delete() error {
	return os.Remove(keyValueStore.path)
}

func (keyValueStore *BoltKeyValueStore) Update(bucket string, key string, value []byte) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		err = b.Put([]byte(key), value)
		if err != nil {
			return err
		}

		return nil
	})

	return err
}

func (keyValueStore *BoltKeyValueStore) GetValue(bucket string, key string) ([]byte, error) {
	var val []byte
	err := keyValueStore.db.View(func(tx *bbolt.Tx) error {
		b := tx.Bucket([]byte(bucket))

		if b == nil {
			return bbolt.ErrBucketNotFound
		}
		val = b.Get([]byte(key))

		return nil
	})

	if err != nil {
		return nil, err
	}

	return val, nil
}

func (keyValueStore *BoltKeyValueStore) DeleteKey(bucket string, key string) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		b := tx.Bucket([]byte(bucket))
		err := b.Delete([]byte(key))

		return err
	})

	return err
}

func (keyValueStore *BoltKeyValueStore) DeleteBucket(bucket string) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		err := tx.DeleteBucket([]byte(bucket))

		return err
	})

	return err
}

func (keyValueStore *BoltKeyValueStore) UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		err = b.Put([]byte(VideoKey), []byte(video))
		if err != nil {
			return err
		}

		pos := make([]byte, 8)
		binary.LittleEndian.PutUint64(pos, position)
		err = b.Put([]byte(PositionKey), pos)
		if err != nil {
			return err
		}

		err = b.Put([]byte(PlaylistKey), playlist)
		if err != nil {
			return err
		}

		return nil
	})

	return err
}

func (keyValueStore *BoltKeyValueStore) GetAll(bucket string) (map[string][]byte, error) {
	bucketValues := make(map[string][]byte, 0)
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		b.ForEach(func(k, v []byte) error {
			bucketValues[string(k[:])] = v

			return nil
		})

		return nil
	})

	if err != nil {
		return nil, err
	}

	return bucketValues, nil
}
