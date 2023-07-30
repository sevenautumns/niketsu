package db

import (
	"encoding/binary"
	"os"
	"testing"

	"github.com/stretchr/testify/require"
)

const (
	validDBPath    = ".db"
	invalidDBPath  = "somepath/.db" // make sure path does not exist
	emptyDBPath    = ""
	validTimeout   = 2
	invalidTimeout = 0
	validBucket    = "bucket"
	emptyBucket    = ""
	otherBucket    = "otherBucket"
	validKey       = "key"
	emptyKey       = ""
	otherKey       = "otherKey"
	validVideo     = "test"
	validPosition  = 0
)

var (
	validValue = []byte("value")
)

func TestValidNewDBManager(t *testing.T) {
	_, err := NewDBManager(validDBPath, validTimeout)
	require.NoError(t, err)
}

func TestInvalidNewDBManager(t *testing.T) {
	_, err := NewDBManager(validDBPath, invalidTimeout)
	require.Error(t, err)

	// invalid paths are implicitly checked when calling Open()
	_, err = NewDBManager(invalidDBPath, validTimeout)
	require.NoError(t, err)
}

func TestValidOpen(t *testing.T) {
	dbManager, err := NewDBManager(validDBPath, validTimeout)
	require.NoError(t, err)
	require.NoFileExists(t, validDBPath)

	err = dbManager.Open()
	require.NoError(t, err)
	require.FileExists(t, validDBPath)

	t.Cleanup(func() {
		os.Remove(validDBPath)
	})
}

func TestInvalidOpen(t *testing.T) {
	dbManager, err := NewDBManager(invalidDBPath, validTimeout)
	require.NoError(t, err)
	err = dbManager.Open()
	require.Error(t, err)
	require.NoFileExists(t, invalidDBPath)

	t.Cleanup(func() {
		os.Remove(validDBPath)
	})
}

func TestValidClose(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	err := dbManager.Close()
	require.NoError(t, err)
}

func TestInvalidClose(t *testing.T) {
	dbManager, err := NewDBManager(validDBPath, validTimeout)
	require.NoError(t, err)
	err = dbManager.Close()
	require.Error(t, err)
}

func createDBManager(t *testing.T, path string, timeout uint64) DBManager {
	dbManager, err := NewDBManager(path, timeout)
	require.NoError(t, err)

	err = dbManager.Open()
	if err != nil {
		t.Fail()
	}

	t.Cleanup(func() {
		os.Remove(path)
	})
	return dbManager
}

func TestDBNotOpen(t *testing.T) {
	db := createDBManager(t, validDBPath, validTimeout)

	// add initial value
	err := db.Update(validBucket, validKey, validValue)
	if err != nil {
		t.Fatal("Failed to open database")
	}
	db.Close()

	err = db.Update(validBucket, validKey, validValue)
	require.Error(t, err)

	_, err = db.GetValue(validBucket, validKey)
	require.Error(t, err)

	err = db.DeleteKey(validBucket, validKey)
	require.Error(t, err)

	err = db.DeleteBucket(validBucket)
	require.Error(t, err)

	err = db.UpdatePlaylist(validBucket, validValue, validVideo, validPosition)
	require.Error(t, err)

	_, err = db.GetAll(validBucket)
	require.Error(t, err)
}

// Since update and get are intertwined, we test both at once
func TestUpdateAndGetValue(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	testUpdateAndGetValueValid(t, dbManager, validBucket, validKey)
	testUpdateAndGetValueValid(t, dbManager, otherBucket, validKey)
	testUpdateAndGetValueValid(t, dbManager, validBucket, otherKey)
}

func testUpdateAndGetValueValid(t *testing.T, dbManager DBManager, bucket string, key string) {
	err := dbManager.Update(bucket, key, validValue)
	require.NoError(t, err)
	testGetValue(t, dbManager, bucket, key, validValue)
}

func testGetValue(t *testing.T, dbManager DBManager, bucket string, key string, value []byte) {
	actualValue, err := dbManager.GetValue(bucket, key)
	require.NoError(t, err)
	require.Equal(t, value, actualValue)
}

func TestFailedGet(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	err := dbManager.Update(validBucket, validKey, validValue)
	if err != nil {
		t.Fatal("Failed to update database")
	}

	testInvalidGetBucket(t, dbManager)
	testInvalidGetKey(t, dbManager)
}

func testInvalidGetBucket(t *testing.T, dbManager DBManager) {
	_, err := dbManager.GetValue(otherBucket, validKey)
	require.Error(t, err)

	_, err = dbManager.GetValue(emptyBucket, validKey)
	require.Error(t, err)
}

func testInvalidGetKey(t *testing.T, dbManager DBManager) {
	value, err := dbManager.GetValue(validBucket, emptyKey)
	require.NoError(t, err)
	require.Nil(t, value)

	value, err = dbManager.GetValue(validBucket, otherKey)
	require.NoError(t, err)
	require.Nil(t, value)
}

func TestFailedUpdate(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	testInvalidUpdateBucket(t, dbManager)
	testInvalidUpdateKey(t, dbManager)
}

func testInvalidUpdateBucket(t *testing.T, dbManager DBManager) {
	err := dbManager.Update(emptyBucket, validKey, validValue)
	require.Error(t, err)
}

func testInvalidUpdateKey(t *testing.T, dbManager DBManager) {
	err := dbManager.Update(validBucket, emptyKey, validValue)
	require.Error(t, err)
}

func TestDeleteKey(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	testValidDeleteKey(t, dbManager, validKey)
	testValidDeleteKey(t, dbManager, otherKey)

	// key should not exist w\o update
	testInvalidDeleteKey(t, dbManager, validKey)
	testInvalidDeleteKey(t, dbManager, otherKey)
	testInvalidDeleteKey(t, dbManager, emptyKey)

	err := dbManager.DeleteKey(validBucket, emptyKey)
	require.NoError(t, err)
}

func testValidDeleteKey(t *testing.T, dbManager DBManager, key string) {
	err := dbManager.Update(validBucket, key, validValue)
	require.NoError(t, err)
	err = dbManager.DeleteKey(validBucket, key)
	require.NoError(t, err)
}

func testInvalidDeleteKey(t *testing.T, dbManager DBManager, key string) {
	err := dbManager.DeleteKey(validBucket, key)
	require.NoError(t, err)
	require.Nil(t, err)
}

func TestDeleteBucket(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	testValidDeleteBucket(t, dbManager, validBucket)
	testValidDeleteBucket(t, dbManager, otherBucket)

	// key should not exist w\o update
	testInvalidDeleteBucket(t, dbManager, validKey)
	testInvalidDeleteBucket(t, dbManager, otherKey)
	testInvalidDeleteBucket(t, dbManager, emptyKey)
}

func testValidDeleteBucket(t *testing.T, dbManager DBManager, bucket string) {
	err := dbManager.Update(bucket, validKey, validValue)
	require.NoError(t, err)
	err = dbManager.DeleteBucket(bucket)
	require.NoError(t, err)
}

func testInvalidDeleteBucket(t *testing.T, dbManager DBManager, bucket string) {
	err := dbManager.DeleteBucket(bucket)
	require.Error(t, err)
}

func TestUpdatePlaylist(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	testValidUpdatePlaylist(t, dbManager)
	testInvalidUpdatePlaylist(t, dbManager)
}

func testValidUpdatePlaylist(t *testing.T, dbManager DBManager) {
	err := dbManager.UpdatePlaylist(validBucket, validValue, validVideo, validPosition)
	require.NoError(t, err)
	testGetValue(t, dbManager, validBucket, PlaylistKey, validValue)
	testGetValue(t, dbManager, validBucket, VideoKey, []byte(validVideo))
	pos := make([]byte, 8)
	binary.LittleEndian.PutUint64(pos, validPosition)
	testGetValue(t, dbManager, validBucket, PositionKey, pos)
}

func testInvalidUpdatePlaylist(t *testing.T, dbManager DBManager) {
	err := dbManager.UpdatePlaylist(emptyBucket, validValue, validVideo, validPosition)
	require.Error(t, err)
}

func TestGetAll(t *testing.T) {
	dbManager := createDBManager(t, validDBPath, validTimeout)
	testValidGetAll(t, dbManager)
	testEmptyGetAll(t, dbManager)
}

func testValidGetAll(t *testing.T, dbManager DBManager) {
	testRoomConfigsEqual(t, dbManager, validBucket, map[string][]byte{})

	writeRoomConfig(t, dbManager, validBucket, validKey, validValue)
	testRoomConfigsEqual(t, dbManager, validBucket, map[string][]byte{
		validKey: validValue,
	})

	writeRoomConfig(t, dbManager, validBucket, otherKey, validValue)
	testRoomConfigsEqual(t, dbManager, validBucket, map[string][]byte{
		validKey: validValue,
		otherKey: validValue,
	})
}

func testRoomConfigsEqual(t *testing.T, dbManager DBManager, bucket string, expectedRoomConfigs map[string][]byte) {
	roomConfigs, err := dbManager.GetAll(validBucket)
	require.NoError(t, err)
	require.Equal(t, expectedRoomConfigs, roomConfigs)
}

func writeRoomConfig(t *testing.T, dbManager DBManager, bucket string, key string, values []byte) {
	err := dbManager.Update(bucket, key, values)
	if err != nil {
		t.Fatal("Update key/value transaction for room configurations failed")
	}
}

func testEmptyGetAll(t *testing.T, dbManager DBManager) {
	roomConfigs, err := dbManager.GetAll(emptyBucket)
	require.Error(t, err)
	require.Nil(t, roomConfigs)
}
